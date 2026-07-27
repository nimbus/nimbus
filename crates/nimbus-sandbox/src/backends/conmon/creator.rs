//! Owned lifecycle for an asynchronous conmon creator command.
//!
//! A runtime-state probe cannot prove cleanup is safe while the command that
//! creates that runtime may still be alive. This adapter retains the child,
//! contains its process group, and authenticates the separate conmon PID
//! receipt before it acknowledges quiescence.

use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt as _;

use crate::backends::conmon::lifecycle::{read_pid, remove_if_exists};
use crate::backends::oci::command::CommandSpec;
use crate::backends::poll::poll_until_deadline;
use crate::error::{Result, SandboxError};
use crate::process::pid_is_alive;

const CREATOR_QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(2);
const CREATOR_QUIESCENCE_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CreatorContainmentPhase {
    /// The unreaped exact child still pins its PID and process-group identity.
    LeaderRetained,
    /// The child was reaped before group absence was proven. The numeric
    /// process-group id is no longer authenticated and may only be observed.
    LeaderReaped,
    /// The exact child was reaped and its process group was observed absent.
    Quiesced,
}

/// Exact process ownership for one live conmon creator attempt.
pub(crate) struct OwnedConmonCreator {
    child: Child,
    /// Exact provider receipt prepared before this creator attempt. Cleanup
    /// may consume only this path; accepting an arbitrary dead-PID file would
    /// manufacture provider-absence evidence for the wrong attempt.
    conmon_pidfile: PathBuf,
    #[cfg(unix)]
    process_group: i32,
    /// Attempt-scoped proof that this exact creator's containment was reaped
    /// and its process group was observed absent. Once established, the
    /// retained numeric group ID must never be signalled again because the
    /// operating system may recycle it.
    containment_phase: CreatorContainmentPhase,
    #[cfg(test)]
    inject_cancellation_ack_loss_once: bool,
}

impl OwnedConmonCreator {
    #[cfg(test)]
    pub(crate) fn spawn(command: &CommandSpec) -> Result<Self> {
        let receipt =
            std::env::temp_dir().join(format!("nimbus-conmon-test-receipt-{}", ulid::Ulid::new()));
        Self::spawn_with_pid_receipt(command, &receipt)
    }

    pub(crate) fn spawn_with_pid_receipt(
        command: &CommandSpec,
        conmon_pidfile: &Path,
    ) -> Result<Self> {
        prepare_pid_receipt_for_new_attempt(conmon_pidfile)?;
        let mut command = command.as_command();
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        command.process_group(0);
        let child = command
            .spawn()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to spawn owned sandbox creator command {}: {error}",
                    command.get_program().to_string_lossy()
                ),
            })?;
        #[cfg(unix)]
        let process_group =
            i32::try_from(child.id()).map_err(|_| SandboxError::OperationFailed {
                message: format!(
                    "sandbox creator PID {} cannot identify a process group",
                    child.id()
                ),
            })?;
        Ok(Self {
            child,
            conmon_pidfile: conmon_pidfile.to_path_buf(),
            #[cfg(unix)]
            process_group,
            containment_phase: CreatorContainmentPhase::LeaderRetained,
            #[cfg(test)]
            inject_cancellation_ack_loss_once: false,
        })
    }

    #[cfg(test)]
    fn inject_cancellation_ack_loss_once(&mut self) {
        self.inject_cancellation_ack_loss_once = true;
    }

    /// Cancel the exact creator containment, reap it, and authenticate the
    /// daemon receipt before cleanup authority can advance.
    pub(crate) fn cancel_and_confirm_quiesced(&mut self) -> Result<()> {
        let conmon_pidfile = self.conmon_pidfile.clone();
        let mut errors = Vec::new();
        let containment_confirmed = match self.cancel_containment_and_reap() {
            Ok(()) => true,
            Err(error) => {
                errors.push(error.to_string());
                false
            }
        };
        #[cfg(test)]
        let containment_confirmed = if std::mem::take(&mut self.inject_cancellation_ack_loss_once) {
            errors.push("injected creator cancellation acknowledgement loss".to_owned());
            false
        } else {
            containment_confirmed
        };
        match conmon_pidfile.try_exists() {
            Ok(true) => match read_pid(&conmon_pidfile) {
                Ok(pid) if pid_is_alive(pid) => errors.push(format!(
                    "creator receipt {} names live PID {pid}; authority remains fenced",
                    conmon_pidfile.display()
                )),
                Ok(_) if containment_confirmed => {
                    if let Err(error) = remove_if_exists(&conmon_pidfile) {
                        errors.push(error.to_string());
                    }
                }
                Ok(_) => {}
                Err(error) => errors.push(error.to_string()),
            },
            Ok(false) => errors.push(format!(
                "creator receipt {} is absent after a spawned attempt; an escaped provider \
                 handoff is ambiguous and authority remains fenced",
                conmon_pidfile.display()
            )),
            Err(error) => errors.push(format!(
                "cannot inspect creator receipt {}: {error}; authority remains fenced",
                conmon_pidfile.display()
            )),
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "cannot confirm sandbox creator quiescence: {}",
                    errors.join("; ")
                ),
            })
        }
    }

    #[cfg(unix)]
    pub(crate) fn cancel_containment_and_reap(&mut self) -> Result<()> {
        if self.containment_phase == CreatorContainmentPhase::Quiesced {
            return Ok(());
        }

        let mut errors = Vec::new();
        if self.containment_phase == CreatorContainmentPhase::LeaderRetained {
            // SAFETY: the unreaped child was spawned into a fresh process
            // group whose ID is its retained PID. The retained child pins that
            // numeric identity until reap, so a negative target addresses only
            // this exact creator group.
            if unsafe { libc::kill(-self.process_group, libc::SIGKILL) } != 0
                && !matches!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::ESRCH)
                )
            {
                errors.push(format!(
                    "failed to signal sandbox creator process group {}: {}",
                    self.process_group,
                    std::io::Error::last_os_error()
                ));
            }
            if let Err(error) = self.reap_child_until(
                CREATOR_QUIESCENCE_TIMEOUT,
                "cancelled sandbox creator process",
            ) {
                errors.push(error.to_string());
            }
        }
        match poll_until_deadline(
            Some(Instant::now() + CREATOR_QUIESCENCE_TIMEOUT),
            CREATOR_QUIESCENCE_POLL_INTERVAL,
            || process_group_is_absent(self.process_group).map(|absent| absent.then_some(())),
        ) {
            Ok(Some(())) => {}
            Ok(None) => errors.push(format!(
                "sandbox creator process group {} remains live after {}ms of cancellation",
                self.process_group,
                CREATOR_QUIESCENCE_TIMEOUT.as_millis()
            )),
            Err(error) => errors.push(error.to_string()),
        }
        if errors.is_empty() {
            self.containment_phase = CreatorContainmentPhase::Quiesced;
            Ok(())
        } else {
            Err(SandboxError::OperationFailed {
                message: errors.join("; "),
            })
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn cancel_containment_and_reap(&mut self) -> Result<()> {
        if self.containment_phase == CreatorContainmentPhase::Quiesced {
            return Ok(());
        }

        let kill_error = self.child.kill().err();
        match self.reap_child_until(
            CREATOR_QUIESCENCE_TIMEOUT,
            "cancelled sandbox creator process",
        ) {
            Ok(_) => {
                self.containment_phase = CreatorContainmentPhase::Quiesced;
                Ok(())
            }
            Err(wait_error) => Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to terminate sandbox creator: {}; failed to reap it: {wait_error}",
                    kill_error
                        .map(|error| error.to_string())
                        .unwrap_or_else(|| "termination was acknowledged".to_owned())
                ),
            }),
        }
    }

    pub(crate) fn reap_after_runtime_observed(&mut self, timeout: Duration) -> Result<()> {
        let status = self.reap_child_until(timeout, "runtime-observed sandbox creator")?;
        let mut errors = Vec::new();
        if !status.success() {
            errors.push(format!(
                "runtime was observed but its owned creator exited with contradictory \
                 status {status}; creator handoff remains pending"
            ));
        }
        if let Err(error) =
            self.confirm_reaped_containment_quiesced(timeout, "runtime-observed sandbox creator")
        {
            errors.push(error.to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(SandboxError::OperationFailed {
                message: errors.join("; "),
            })
        }
    }

    #[cfg(unix)]
    fn confirm_reaped_containment_quiesced(
        &mut self,
        timeout: Duration,
        context: &str,
    ) -> Result<()> {
        if self.containment_phase == CreatorContainmentPhase::Quiesced {
            return Ok(());
        }
        if self.containment_phase == CreatorContainmentPhase::LeaderRetained {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "sandbox creator leader remains unreaped while confirming {context}; creator \
                     handoff remains pending"
                ),
            });
        }

        match poll_until_deadline(
            Some(Instant::now() + timeout),
            CREATOR_QUIESCENCE_POLL_INTERVAL,
            || process_group_is_absent(self.process_group).map(|absent| absent.then_some(())),
        ) {
            Ok(Some(())) => {
                self.containment_phase = CreatorContainmentPhase::Quiesced;
                Ok(())
            }
            Ok(None) => Err(SandboxError::OperationFailed {
                message: format!(
                    "sandbox creator process group {} remains live after {}ms while confirming \
                     {context} containment; creator handoff remains pending",
                    self.process_group,
                    timeout.as_millis()
                ),
            }),
            Err(error) => Err(error),
        }
    }

    #[cfg(not(unix))]
    fn confirm_reaped_containment_quiesced(
        &mut self,
        _timeout: Duration,
        _context: &str,
    ) -> Result<()> {
        self.containment_phase = CreatorContainmentPhase::Quiesced;
        Ok(())
    }

    fn reap_child_until(&mut self, timeout: Duration, context: &str) -> Result<ExitStatus> {
        match poll_until_deadline(
            Some(Instant::now() + timeout),
            CREATOR_QUIESCENCE_POLL_INTERVAL,
            || {
                self.child
                    .try_wait()
                    .map_err(|error| SandboxError::OperationFailed {
                        message: format!("failed to reap {context}: {error}"),
                    })
            },
        )? {
            Some(status) => {
                if self.containment_phase == CreatorContainmentPhase::LeaderRetained {
                    self.containment_phase = CreatorContainmentPhase::LeaderReaped;
                }
                Ok(status)
            }
            None => Err(SandboxError::OperationFailed {
                message: format!(
                    "timed out after {}ms waiting to reap {context}; creator handoff remains \
                     pending",
                    timeout.as_millis()
                ),
            }),
        }
    }
}

fn prepare_pid_receipt_for_new_attempt(conmon_pidfile: &Path) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(conmon_pidfile) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "cannot inspect preexisting creator receipt {} before spawn: {error}; \
                     provider identity remains ambiguous",
                    conmon_pidfile.display()
                ),
            });
        }
    };
    if !metadata.file_type().is_file() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "preexisting creator receipt {} is not a regular file; provider identity \
                 remains ambiguous",
                conmon_pidfile.display()
            ),
        });
    }
    let pid = read_pid(conmon_pidfile)?;
    if pid_is_alive(pid) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "preexisting creator receipt {} names live PID {pid}; refusing to spawn a new \
                 provider attempt",
                conmon_pidfile.display()
            ),
        });
    }
    remove_if_exists(conmon_pidfile)
}

#[cfg(unix)]
fn process_group_is_absent(process_group: i32) -> Result<bool> {
    // SAFETY: signal zero probes only the retained process-group identifier.
    if unsafe { libc::kill(-process_group, 0) } == 0 {
        return Ok(false);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(true),
        Some(libc::EPERM) => Ok(false),
        _ => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to inspect sandbox creator process group {process_group}: {error}"
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::process::CommandExt as _;
    #[cfg(unix)]
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    #[cfg(unix)]
    use crate::backends::conmon::lifecycle::wait_for_path;

    #[cfg(unix)]
    struct EscapedTestProcess {
        child: Child,
    }

    #[cfg(unix)]
    impl EscapedTestProcess {
        fn spawn(receipt: &Path) -> Self {
            let mut command = Command::new("/bin/sh");
            command.args([
                "-c",
                &format!(
                    "printf '%s' \"$$\" > {}; exec sleep 60",
                    shell_words::quote(&receipt.to_string_lossy())
                ),
            ]);
            // SAFETY: this isolated test child has no shared state between fork
            // and exec. It models a provider escaping the creator group.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
            Self {
                child: command
                    .spawn()
                    .expect("escaped provider test process should spawn"),
            }
        }

        fn terminate_and_reap(&mut self) -> std::result::Result<(), String> {
            let _ = self.child.kill();
            poll_until_deadline(
                Some(Instant::now() + CREATOR_QUIESCENCE_TIMEOUT),
                CREATOR_QUIESCENCE_POLL_INTERVAL,
                || {
                    self.child
                        .try_wait()
                        .map(|status| status.map(|_| ()))
                        .map_err(|error| SandboxError::OperationFailed {
                            message: format!(
                                "failed to reap escaped provider test process: {error}"
                            ),
                        })
                },
            )
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                "escaped provider test process was not reaped within the cleanup bound".to_owned()
            })
        }
    }

    #[cfg(unix)]
    impl Drop for EscapedTestProcess {
        fn drop(&mut self) {
            if matches!(self.child.try_wait(), Ok(None)) {
                let _ = self.terminate_and_reap();
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn creator_cancellation_reaps_pre_receipt_process_group() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let entered = temp_dir.path().join("creator-entered");
        let command = CommandSpec::new("/bin/sh").args([
            "-c".to_owned(),
            format!(
                "sleep 60 & descendant=$!; printf '%s' \"$descendant\" > {}; wait \"$descendant\"",
                shell_words::quote(&entered.to_string_lossy())
            ),
        ]);
        let mut creator =
            OwnedConmonCreator::spawn(&command).expect("creator process should spawn");
        assert!(
            wait_for_path(&entered, Duration::from_secs(2)),
            "semantic creator-entry receipt should appear"
        );

        creator
            .cancel_containment_and_reap()
            .expect("owned process group should be cancelled and reaped");
        let descendant = std::fs::read_to_string(&entered)
            .expect("descendant receipt should read")
            .parse::<u32>()
            .expect("descendant receipt should contain a PID");
        assert!(
            !pid_is_alive(descendant),
            "wrapper descendants must be absent before quiescence is acknowledged"
        );
    }

    #[cfg(unix)]
    #[test]
    fn missing_creator_pid_receipt_cannot_authorize_quiescence() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let receipt = temp_dir.path().join("missing-conmon.pid");
        let mut creator = OwnedConmonCreator::spawn_with_pid_receipt(
            &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
            &receipt,
        )
        .expect("owned creator should spawn");

        let error = creator
            .cancel_and_confirm_quiesced()
            .expect_err("an absent provider PID receipt must remain ambiguous");
        assert!(
            error.to_string().contains("receipt")
                && error.to_string().contains("authority remains fenced"),
            "the diagnostic must preserve the ambiguous creator fence: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stale_dead_pid_receipt_cannot_authorize_a_new_creator_attempt() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let receipt = temp_dir.path().join("conmon.pid");
        std::fs::write(&receipt, format!("{}\n", i32::MAX))
            .expect("stale dead provider receipt should persist");
        let mut creator = OwnedConmonCreator::spawn_with_pid_receipt(
            &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
            &receipt,
        )
        .expect("owned creator should spawn");

        let error = creator
            .cancel_and_confirm_quiesced()
            .expect_err("a receipt predating this creator attempt must not prove provider absence");
        assert!(
            error.to_string().contains("receipt")
                && error.to_string().contains("authority remains fenced"),
            "the stale receipt must remain an explicit cleanup fence: {error}"
        );
        assert!(
            !receipt.exists(),
            "the stale receipt must be removed before the new creator can run"
        );
    }

    #[cfg(unix)]
    #[test]
    fn foreign_dead_pid_receipt_cannot_authorize_a_creator_attempt() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let expected_receipt = temp_dir.path().join("conmon.pid");
        let foreign_receipt = temp_dir.path().join("foreign-conmon.pid");
        let mut creator = OwnedConmonCreator::spawn_with_pid_receipt(
            &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
            &expected_receipt,
        )
        .expect("owned creator should spawn");
        std::fs::write(&foreign_receipt, format!("{}\n", i32::MAX))
            .expect("foreign dead provider receipt should persist");

        let error = creator
            .cancel_and_confirm_quiesced()
            .expect_err("a foreign receipt must not authorize this creator attempt");
        assert!(
            error
                .to_string()
                .contains(&expected_receipt.display().to_string())
                && error.to_string().contains("authority remains fenced"),
            "the diagnostic must retain the attempt-scoped receipt authority: {error}"
        );
        assert!(
            foreign_receipt.exists(),
            "a rejected foreign receipt must not be consumed"
        );
        assert!(
            !pid_is_alive(creator.child.id()),
            "the exact creator containment must still be cancelled"
        );
    }

    #[cfg(unix)]
    #[test]
    fn live_preexisting_pid_receipt_blocks_creator_before_effect() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let receipt = temp_dir.path().join("conmon.pid");
        let effect = temp_dir.path().join("creator-effect");
        std::fs::write(&receipt, format!("{}\n", std::process::id()))
            .expect("live provider receipt should persist");
        let command = CommandSpec::new("/bin/sh").args([
            "-c".to_owned(),
            format!("touch {}", shell_words::quote(&effect.to_string_lossy())),
        ]);

        let error = OwnedConmonCreator::spawn_with_pid_receipt(&command, &receipt)
            .err()
            .expect("a live preexisting provider must fence the new attempt");
        assert!(
            error.to_string().contains("names live PID")
                && error.to_string().contains("refusing to spawn"),
            "the refusal must retain exact live-provider evidence: {error}"
        );
        assert!(
            !effect.exists(),
            "receipt authentication must precede the creator command"
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_regular_pid_receipt_blocks_creator_before_effect() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let receipt = temp_dir.path().join("conmon.pid");
        let effect = temp_dir.path().join("creator-effect");
        std::fs::create_dir(&receipt).expect("directory-shaped receipt should exist");
        let command = CommandSpec::new("/bin/sh").args([
            "-c".to_owned(),
            format!("touch {}", shell_words::quote(&effect.to_string_lossy())),
        ]);

        let error = OwnedConmonCreator::spawn_with_pid_receipt(&command, &receipt)
            .err()
            .expect("a non-regular receipt must fence the new attempt");
        assert!(
            error.to_string().contains("is not a regular file")
                && error.to_string().contains("identity remains ambiguous"),
            "the refusal must retain the invalid receipt evidence: {error}"
        );
        assert!(
            !effect.exists(),
            "receipt shape validation must precede the creator command"
        );
    }

    #[cfg(unix)]
    #[test]
    fn escaped_creator_without_pid_receipt_remains_fenced() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let escaped_receipt = temp_dir.path().join("escaped-provider.pid");
        let mut escaped = EscapedTestProcess::spawn(&escaped_receipt);
        assert!(
            wait_for_path(&escaped_receipt, Duration::from_secs(2)),
            "escaped provider semantic receipt should appear"
        );
        let escaped_pid = escaped.child.id();

        let missing_receipt = temp_dir.path().join("missing-conmon.pid");
        let mut creator = OwnedConmonCreator::spawn_with_pid_receipt(
            &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
            &missing_receipt,
        )
        .expect("owned creator should spawn");
        let error = creator
            .cancel_and_confirm_quiesced()
            .expect_err("an escaped provider without a PID receipt must remain ambiguous");

        assert!(
            pid_is_alive(escaped_pid),
            "creator-group cancellation must not imply escaped-provider absence"
        );
        assert!(
            error
                .to_string()
                .contains("escaped provider handoff is ambiguous"),
            "the diagnostic must retain authority for the escaped provider: {error}"
        );
        escaped
            .terminate_and_reap()
            .expect("escaped provider cleanup must remain bounded");
        assert!(!pid_is_alive(escaped_pid));
    }

    #[cfg(unix)]
    #[test]
    fn runtime_observed_creator_is_reaped_before_handoff_success() {
        let mut creator = OwnedConmonCreator::spawn(
            &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exit 0".to_owned()]),
        )
        .expect("successful creator should spawn");
        let pid = creator.child.id() as libc::pid_t;

        creator
            .reap_after_runtime_observed(Duration::from_secs(1))
            .expect("successful intermediary should be reaped");
        let mut status = 0;
        // SAFETY: this probes only the exact child already reaped above.
        assert_eq!(
            unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[cfg(unix)]
    #[test]
    fn transient_creator_cancellation_failure_retains_receipt_for_bounded_retry() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let receipt = temp_dir.path().join("conmon.pid");
        let mut exited = Command::new("/usr/bin/true")
            .spawn()
            .expect("receipt process should spawn");
        let exited_pid = exited.id();
        exited.wait().expect("receipt process should be reaped");
        let receipt_bytes = format!("{exited_pid}\n");
        let mut creator = OwnedConmonCreator::spawn_with_pid_receipt(
            &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
            &receipt,
        )
        .expect("owned creator should spawn");
        std::fs::write(&receipt, &receipt_bytes).expect("dead provider receipt should persist");
        creator.inject_cancellation_ack_loss_once();

        let first = creator
            .cancel_and_confirm_quiesced()
            .expect_err("transient cancellation acknowledgement loss must remain retryable");
        assert!(
            first.to_string().contains("acknowledgement loss"),
            "the transient diagnostic should be preserved: {first}"
        );
        assert_eq!(
            std::fs::read(&receipt).expect("retry evidence must remain"),
            receipt_bytes.as_bytes(),
            "a failed containment acknowledgement must not consume the provider receipt"
        );

        creator
            .cancel_and_confirm_quiesced()
            .expect("same-owner retry should confirm quiescence");
        assert!(
            !receipt.exists(),
            "successful bounded retry should consume the authenticated dead receipt"
        );
    }

    #[cfg(unix)]
    #[test]
    fn receipt_retry_never_signals_after_exact_creator_group_was_proven_absent() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let receipt = temp_dir.path().join("conmon.pid");
        let mut exited = Command::new("/usr/bin/true")
            .spawn()
            .expect("receipt process should spawn");
        let exited_pid = exited.id();
        exited.wait().expect("receipt process should be reaped");
        let mut creator = OwnedConmonCreator::spawn_with_pid_receipt(
            &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
            &receipt,
        )
        .expect("owned creator should spawn");
        std::fs::write(&receipt, format!("{exited_pid}\n"))
            .expect("dead provider receipt should persist");
        creator.inject_cancellation_ack_loss_once();
        creator
            .cancel_and_confirm_quiesced()
            .expect_err("receipt consumption should remain retryable after acknowledgement loss");

        let mut recycled_group = Command::new("/bin/sh");
        recycled_group.args(["-c", "exec sleep 60"]);
        recycled_group.process_group(0);
        let mut recycled_group = recycled_group
            .spawn()
            .expect("recycled process group sentinel should spawn");
        creator.process_group =
            i32::try_from(recycled_group.id()).expect("test process group should fit i32");

        let retry = creator.cancel_and_confirm_quiesced();
        let sentinel_status = recycled_group
            .try_wait()
            .expect("recycled process group sentinel should be inspectable");
        let sentinel_alive = sentinel_status.is_none();
        if sentinel_alive {
            let _ = recycled_group.kill();
            recycled_group
                .wait()
                .expect("live recycled process group sentinel should be reaped");
        }

        assert!(
            sentinel_alive,
            "a confirmed creator attempt must never signal a subsequently recycled process group"
        );
        retry.expect("receipt-only retry should confirm quiescence");
        assert!(
            !receipt.exists(),
            "receipt-only retry should consume the authenticated dead receipt"
        );
    }

    #[cfg(unix)]
    #[test]
    fn runtime_observed_nonzero_creator_exit_fails_closed_after_reap() {
        let mut creator = OwnedConmonCreator::spawn(
            &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exit 23".to_owned()]),
        )
        .expect("failing creator should spawn");

        let error = creator
            .reap_after_runtime_observed(Duration::from_secs(1))
            .expect_err("contradictory creator exit must fail closed");
        assert!(
            error.to_string().contains("contradictory") && error.to_string().contains("status"),
            "the diagnostic must preserve the contradictory exit: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn contradictory_runtime_observed_reap_never_signals_a_recycled_process_group() {
        let mut creator = OwnedConmonCreator::spawn(
            &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exit 23".to_owned()]),
        )
        .expect("failing creator should spawn");
        let error = creator
            .reap_after_runtime_observed(Duration::from_secs(1))
            .expect_err("contradictory creator exit must remain an error");
        assert!(
            error.to_string().contains("contradictory") && error.to_string().contains("status"),
            "the original creator-exit diagnostic must be preserved: {error}"
        );

        let mut recycled_group = Command::new("/bin/sh");
        recycled_group.args(["-c", "exec sleep 60"]);
        recycled_group.process_group(0);
        let mut recycled_group = recycled_group
            .spawn()
            .expect("recycled process group sentinel should spawn");
        creator.process_group =
            i32::try_from(recycled_group.id()).expect("test process group should fit i32");

        let cleanup = creator.cancel_containment_and_reap();
        let sentinel_status = recycled_group
            .try_wait()
            .expect("recycled process group sentinel should be inspectable");
        let sentinel_alive = sentinel_status.is_none();
        if sentinel_alive {
            let _ = recycled_group.kill();
            recycled_group
                .wait()
                .expect("live recycled process group sentinel should be reaped");
        }

        assert!(
            sentinel_alive,
            "runtime-observed reap must prevent later signals to a recycled process group"
        );
        cleanup.expect("same-attempt cleanup should reuse the exact quiescence proof");
    }

    #[cfg(unix)]
    #[test]
    fn runtime_observed_reap_fails_closed_while_creator_group_descendant_remains() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let descendant_receipt = temp_dir.path().join("creator-descendant.pid");
        let command = CommandSpec::new("/bin/sh").args([
            "-c".to_owned(),
            format!(
                "sleep 60 & descendant=$!; printf '%s' \"$descendant\" > {}; exit 0",
                shell_words::quote(&descendant_receipt.to_string_lossy())
            ),
        ]);
        let mut creator =
            OwnedConmonCreator::spawn(&command).expect("creator process should spawn");
        assert!(
            wait_for_path(&descendant_receipt, Duration::from_secs(2)),
            "creator descendant receipt should appear"
        );
        let descendant = std::fs::read_to_string(&descendant_receipt)
            .expect("descendant receipt should read")
            .parse::<u32>()
            .expect("descendant receipt should contain a PID");

        let error = creator
            .reap_after_runtime_observed(Duration::from_millis(40))
            .expect_err("a live creator-group descendant must retain the cleanup fence");
        assert!(
            error.to_string().contains("process group")
                && error.to_string().contains("handoff remains pending"),
            "the diagnostic must preserve the containment fence: {error}"
        );
        assert!(
            pid_is_alive(descendant),
            "runtime observation must not itself cancel a retained creator descendant"
        );

        let cleanup = creator
            .cancel_containment_and_reap()
            .expect_err("post-reap numeric group identity must remain observation-only");
        assert!(
            cleanup.to_string().contains("process group")
                && cleanup.to_string().contains("remains live"),
            "post-reap cleanup must preserve the identity fence: {cleanup}"
        );
        assert!(
            pid_is_alive(descendant),
            "post-reap cleanup must not signal through an unauthenticated numeric group id"
        );
        // SAFETY: the receipt was written by the exact test descendant.
        assert_eq!(unsafe { libc::kill(descendant as i32, libc::SIGKILL) }, 0);
        creator
            .cancel_containment_and_reap()
            .expect("observation-only retry should accept externally established group absence");
        assert!(
            !pid_is_alive(descendant),
            "external descendant retirement must establish group absence before success"
        );
    }

    #[cfg(unix)]
    #[test]
    fn post_reap_unconfirmed_group_never_signals_recycled_numeric_group() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let descendant_receipt = temp_dir.path().join("creator-descendant.pid");
        let command = CommandSpec::new("/bin/sh").args([
            "-c".to_owned(),
            format!(
                "sleep 60 & descendant=$!; printf '%s' \"$descendant\" > {}; exit 0",
                shell_words::quote(&descendant_receipt.to_string_lossy())
            ),
        ]);
        let mut creator =
            OwnedConmonCreator::spawn(&command).expect("creator process should spawn");
        let original_group = creator.process_group;
        assert!(
            wait_for_path(&descendant_receipt, Duration::from_secs(2)),
            "creator descendant receipt should appear"
        );
        let descendant = std::fs::read_to_string(&descendant_receipt)
            .expect("descendant receipt should read")
            .parse::<u32>()
            .expect("descendant receipt should contain a PID");

        creator
            .reap_after_runtime_observed(Duration::from_millis(40))
            .expect_err("the live descendant must leave group absence unconfirmed");
        // SAFETY: the receipt was written by the exact test descendant.
        assert_eq!(unsafe { libc::kill(descendant as i32, libc::SIGKILL) }, 0);
        assert!(
            matches!(
                poll_until_deadline(
                    Some(Instant::now() + CREATOR_QUIESCENCE_TIMEOUT),
                    CREATOR_QUIESCENCE_POLL_INTERVAL,
                    || process_group_is_absent(original_group).map(|absent| absent.then_some(())),
                ),
                Ok(Some(()))
            ),
            "the original creator group must drain before the numeric-id substitution"
        );

        let mut recycled_group = Command::new("/bin/sh");
        recycled_group.args(["-c", "exec sleep 60"]);
        recycled_group.process_group(0);
        let mut recycled_group = recycled_group
            .spawn()
            .expect("recycled process group sentinel should spawn");
        creator.process_group =
            i32::try_from(recycled_group.id()).expect("test process group should fit i32");

        let cleanup = creator.cancel_containment_and_reap();
        let sentinel_status = recycled_group
            .try_wait()
            .expect("recycled process group sentinel should be inspectable");
        let sentinel_alive = sentinel_status.is_none();
        if sentinel_alive {
            let _ = recycled_group.kill();
            recycled_group
                .wait()
                .expect("live recycled process group sentinel should be reaped");
        }

        assert!(
            sentinel_alive,
            "post-reap recovery must never signal a recycled numeric process group"
        );
        let error = cleanup.expect_err(
            "a live group under the recycled number must retain the identity ambiguity fence",
        );
        assert!(
            error.to_string().contains("process group")
                && error.to_string().contains("remains live"),
            "the diagnostic must preserve the observation-only fence: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn runtime_observed_live_creator_wait_is_bounded() {
        let mut creator = OwnedConmonCreator::spawn(
            &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
        )
        .expect("live creator should spawn");

        let error = creator
            .reap_after_runtime_observed(Duration::from_millis(40))
            .expect_err("a live intermediary must time out rather than block");
        assert!(error.to_string().contains("handoff remains pending"));
        creator
            .cancel_containment_and_reap()
            .expect("the bounded test must clean up its exact creator");
    }
}
