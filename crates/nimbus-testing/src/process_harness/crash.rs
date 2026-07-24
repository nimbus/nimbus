use super::{
    Checkpoint, CommandMessage, HarnessFailure, ProcessDiagnostic, ProcessHarnessError,
    ProcessRoleSpec, ROLE_ENV, STATE_ROOT_ENV, SpawnedRole, emit_checkpoint, expect_command,
    validate_semantic_token,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct CrashCutChildContext {
    role: String,
    state_root: PathBuf,
}

impl CrashCutChildContext {
    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    /// Acknowledge the exact named crash boundary and remain parked until the
    /// parent kills this process.
    ///
    /// Call this at the fault point itself, after the state/effect whose crash
    /// semantics are under test. It intentionally never returns.
    pub fn reach_boundary(&self, boundary: &str) -> Result<(), String> {
        validate_semantic_token("boundary", boundary)?;
        emit_checkpoint(Checkpoint::Boundary(boundary.to_owned()))?;
        loop {
            std::thread::park();
        }
    }
}

/// Run the crash-child side of a named-boundary persistence test.
///
/// The operation must call [`CrashCutChildContext::reach_boundary`] at the
/// exact fault point. Returning normally is an error because the parent would
/// otherwise have no proof that it killed at the requested boundary.
pub fn run_crash_cut_child(
    operation: impl FnOnce(&CrashCutChildContext) -> Result<(), String>,
) -> Result<(), String> {
    let context = child_context_from_env()?;
    emit_checkpoint(Checkpoint::Ready)?;
    expect_command(CommandMessage::Run)?;
    operation(&context)?;
    Err("crash-cut child returned before reporting a named boundary".to_owned())
}

/// Run the fresh-process recovery side of a crash-cut persistence test.
pub fn run_crash_recovery_child(
    recovery: impl FnOnce(&CrashCutChildContext) -> Result<String, String>,
) -> Result<(), String> {
    let context = child_context_from_env()?;
    emit_checkpoint(Checkpoint::Ready)?;
    expect_command(CommandMessage::Inspect)?;
    let observation = recovery(&context)?;
    validate_semantic_token("recovery observation", &observation)?;
    emit_checkpoint(Checkpoint::Recovered(observation))
}

#[derive(Debug)]
pub struct SubprocessCrashCutHarness {
    checkpoint_timeout: Duration,
}

impl SubprocessCrashCutHarness {
    pub fn new(checkpoint_timeout: Duration) -> Self {
        Self { checkpoint_timeout }
    }

    pub fn run(
        &self,
        state_root: &Path,
        boundary: &str,
        expected_recovery: &str,
        crash_process: ProcessRoleSpec,
        recovery_process: ProcessRoleSpec,
    ) -> Result<SubprocessCrashCutResult, ProcessHarnessError> {
        self.validate(
            boundary,
            expected_recovery,
            &crash_process,
            &recovery_process,
        )?;
        let state_root = canonical_state_root(state_root)?;

        let mut crash = SpawnedRole::spawn(crash_process, &state_root).map_err(|failure| {
            ProcessHarnessError {
                failure,
                diagnostics: Vec::new(),
            }
        })?;
        let crash_result = self.run_crash_phase(&mut crash, boundary);
        let crash_diagnostic = match crash_result {
            Ok(diagnostic) => diagnostic,
            Err(failure) => {
                return Err(ProcessHarnessError {
                    failure,
                    diagnostics: vec![crash.cleanup()],
                });
            }
        };

        let mut recovery = match SpawnedRole::spawn(recovery_process, &state_root) {
            Ok(recovery) => recovery,
            Err(failure) => {
                return Err(ProcessHarnessError {
                    failure,
                    diagnostics: vec![crash_diagnostic],
                });
            }
        };
        match self.run_recovery_phase(&mut recovery, expected_recovery) {
            Ok(observation) => {
                let recovery_diagnostic = recovery.cleanup();
                if recovery_diagnostic.successful() != Some(true) {
                    return Err(ProcessHarnessError {
                        failure: HarnessFailure::Protocol {
                            role: recovery_diagnostic.role().to_owned(),
                            message: format!(
                                "recovery acknowledged observation but terminated with {}",
                                recovery_diagnostic.status().unwrap_or("unknown status")
                            ),
                        },
                        diagnostics: vec![crash_diagnostic, recovery_diagnostic],
                    });
                }
                Ok(SubprocessCrashCutResult {
                    boundary: boundary.to_owned(),
                    observation,
                    crash: crash_diagnostic,
                    recovery: recovery_diagnostic,
                })
            }
            Err(failure) => Err(ProcessHarnessError {
                failure,
                diagnostics: vec![crash_diagnostic, recovery.cleanup()],
            }),
        }
    }

    fn validate(
        &self,
        boundary: &str,
        expected_recovery: &str,
        crash: &ProcessRoleSpec,
        recovery: &ProcessRoleSpec,
    ) -> Result<(), ProcessHarnessError> {
        if self.checkpoint_timeout.is_zero() {
            return Err(configuration_error(
                "checkpoint timeout must be greater than zero",
            ));
        }
        validate_semantic_token("boundary", boundary).map_err(configuration_error)?;
        validate_semantic_token("recovery observation", expected_recovery)
            .map_err(configuration_error)?;
        if crash.role.is_empty() || recovery.role.is_empty() {
            return Err(configuration_error("both process roles must be non-empty"));
        }
        if crash.role == recovery.role {
            return Err(configuration_error(format!(
                "crash and recovery roles must be unique; both were {:?}",
                crash.role
            )));
        }
        Ok(())
    }

    fn run_crash_phase(
        &self,
        crash: &mut SpawnedRole,
        expected_boundary: &str,
    ) -> Result<ProcessDiagnostic, HarnessFailure> {
        let deadline = Instant::now() + self.checkpoint_timeout;
        let checkpoint = crash.receive_checkpoint(deadline, "ready")?;
        if checkpoint != Checkpoint::Ready {
            return Err(HarnessFailure::UnexpectedCheckpoint {
                role: crash.role.clone(),
                expected: "ready".to_owned(),
                actual: checkpoint.to_string(),
            });
        }
        crash.send_command(CommandMessage::Run)?;

        let deadline = Instant::now() + self.checkpoint_timeout;
        let checkpoint =
            crash.receive_checkpoint(deadline, &format!("boundary:{expected_boundary}"))?;
        match checkpoint {
            Checkpoint::Boundary(actual) if actual == expected_boundary => {
                crash.kill_at_acknowledged_boundary()
            }
            actual => Err(HarnessFailure::UnexpectedCheckpoint {
                role: crash.role.clone(),
                expected: format!("boundary:{expected_boundary}"),
                actual: actual.to_string(),
            }),
        }
    }

    fn run_recovery_phase(
        &self,
        recovery: &mut SpawnedRole,
        expected_recovery: &str,
    ) -> Result<String, HarnessFailure> {
        let deadline = Instant::now() + self.checkpoint_timeout;
        let checkpoint = recovery.receive_checkpoint(deadline, "ready")?;
        if checkpoint != Checkpoint::Ready {
            return Err(HarnessFailure::UnexpectedCheckpoint {
                role: recovery.role.clone(),
                expected: "ready".to_owned(),
                actual: checkpoint.to_string(),
            });
        }
        recovery.send_command(CommandMessage::Inspect)?;

        let deadline = Instant::now() + self.checkpoint_timeout;
        let checkpoint =
            recovery.receive_checkpoint(deadline, &format!("recovered:{expected_recovery}"))?;
        let observation = match checkpoint {
            Checkpoint::Recovered(actual) if actual == expected_recovery => actual,
            actual => {
                return Err(HarnessFailure::UnexpectedCheckpoint {
                    role: recovery.role.clone(),
                    expected: format!("recovered:{expected_recovery}"),
                    actual: actual.to_string(),
                });
            }
        };
        let deadline = Instant::now() + self.checkpoint_timeout;
        recovery.wait_for_stdout_eof(deadline)?;
        recovery.wait_for_exit()?;
        Ok(observation)
    }
}

impl SpawnedRole {
    fn kill_at_acknowledged_boundary(&mut self) -> Result<ProcessDiagnostic, HarnessFailure> {
        self.stdin.take();
        let Some(mut child) = self.child.take() else {
            return Err(HarnessFailure::Protocol {
                role: self.role.clone(),
                message: "crash child handle was already consumed at boundary".to_owned(),
            });
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                self.status = Some(status);
                self.cleanup_result = Some("exited-before-parent-crash-and-reaped".to_owned());
                Err(HarnessFailure::EarlyExit {
                    role: self.role.clone(),
                    expected: "parent kill after acknowledged boundary".to_owned(),
                })
            }
            Ok(None) => {
                if let Err(error) = child.kill() {
                    self.child = Some(child);
                    return Err(HarnessFailure::Protocol {
                        role: self.role.clone(),
                        message: format!(
                            "failed to kill crash child after acknowledged boundary: {error}"
                        ),
                    });
                }
                let status = child.wait().map_err(|error| HarnessFailure::Protocol {
                    role: self.role.clone(),
                    message: format!(
                        "failed to reap crash child after acknowledged boundary: {error}"
                    ),
                })?;
                self.status = Some(status);
                self.cleanup_result = Some("killed-at-boundary-and-reaped".to_owned());
                Ok(self.cleanup())
            }
            Err(error) => {
                self.child = Some(child);
                Err(HarnessFailure::Protocol {
                    role: self.role.clone(),
                    message: format!("failed to inspect crash child before boundary kill: {error}"),
                })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubprocessCrashCutResult {
    boundary: String,
    observation: String,
    crash: ProcessDiagnostic,
    recovery: ProcessDiagnostic,
}

impl SubprocessCrashCutResult {
    pub fn boundary(&self) -> &str {
        &self.boundary
    }

    pub fn observation(&self) -> &str {
        &self.observation
    }

    pub fn crash_diagnostic(&self) -> &ProcessDiagnostic {
        &self.crash
    }

    pub fn recovery_diagnostic(&self) -> &ProcessDiagnostic {
        &self.recovery
    }
}

fn child_context_from_env() -> Result<CrashCutChildContext, String> {
    let role = std::env::var(ROLE_ENV)
        .map_err(|error| format!("missing or invalid child role in {ROLE_ENV}: {error}"))?;
    let state_root = std::env::var_os(STATE_ROOT_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing child state root in {STATE_ROOT_ENV}"))?;
    Ok(CrashCutChildContext { role, state_root })
}

fn canonical_state_root(state_root: &Path) -> Result<PathBuf, ProcessHarnessError> {
    fs::create_dir_all(state_root).map_err(|error| {
        configuration_error(format!(
            "state root {} could not be created: {error}",
            state_root.display()
        ))
    })?;
    fs::canonicalize(state_root).map_err(|error| {
        configuration_error(format!(
            "state root {} could not be canonicalized: {error}",
            state_root.display()
        ))
    })
}

fn configuration_error(message: impl Into<String>) -> ProcessHarnessError {
    ProcessHarnessError {
        failure: HarnessFailure::InvalidConfiguration(message.into()),
        diagnostics: Vec::new(),
    }
}

#[cfg(test)]
fn emit_checkpoint_for_test_ready() -> Result<(), String> {
    emit_checkpoint(Checkpoint::Ready)
}

#[cfg(test)]
fn expect_inspect_for_test() -> Result<(), String> {
    expect_command(CommandMessage::Inspect)
}

#[cfg(test)]
mod tests {
    use super::{SubprocessCrashCutHarness, run_crash_cut_child, run_crash_recovery_child};
    use crate::{ProcessDiagnostic, ProcessHarnessError, ProcessRoleSpec};
    use std::fs::{self, File};
    use std::io::Write;
    use std::time::Duration;

    const CHILD_TEST: &str = "process_harness::crash::tests::crash_cut_protocol_child";
    const MODE_ENV: &str = "NIMBUS_PROCESS_CRASH_HARNESS_TEST_MODE";
    const EXPECTED_BOUNDARY: &str = "network.store.after-state-and-effect-sync";
    const EXPECTED_RECOVERY: &str = "state-committed:effect-created";

    #[test]
    fn kills_only_at_exact_boundary_then_recovers_same_root() {
        let root = tempfile::tempdir().expect("state root should exist");
        let result = harness()
            .run(
                root.path(),
                EXPECTED_BOUNDARY,
                EXPECTED_RECOVERY,
                child("crash-writer", "write-and-boundary"),
                child("fresh-recovery", "recover"),
            )
            .expect("crash cut and recovery should succeed");

        assert_eq!(result.boundary(), EXPECTED_BOUNDARY);
        assert_eq!(result.observation(), EXPECTED_RECOVERY);
        assert_eq!(
            result.crash_diagnostic().cleanup(),
            "killed-at-boundary-and-reaped"
        );
        assert_eq!(result.crash_diagnostic().successful(), Some(false));
        let expected_checkpoint = format!("boundary:{EXPECTED_BOUNDARY}");
        assert_eq!(
            result.crash_diagnostic().last_checkpoint(),
            Some(expected_checkpoint.as_str())
        );
        assert_eq!(result.recovery_diagnostic().successful(), Some(true));
        assert_eq!(
            fs::read_to_string(root.path().join("state")).expect("durable state should remain"),
            "committed"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("effect")).expect("effect evidence should remain"),
            "created"
        );
    }

    #[test]
    fn wrong_boundary_fails_before_parent_crash_and_reports_actual_boundary() {
        let root = tempfile::tempdir().expect("state root should exist");
        let error = harness()
            .run(
                root.path(),
                EXPECTED_BOUNDARY,
                EXPECTED_RECOVERY,
                child("wrong-boundary", "wrong-boundary"),
                child("fresh-recovery", "recover"),
            )
            .expect_err("wrong boundary must not be accepted");

        assert_error_report(
            &error,
            &format!(
                "reached checkpoint \"boundary:network.store.wrong\"; expected \"boundary:{EXPECTED_BOUNDARY}\""
            ),
        );
        let crash = diagnostic(&error, "wrong-boundary");
        assert_eq!(
            crash.last_checkpoint(),
            Some("boundary:network.store.wrong")
        );
        assert_eq!(crash.cleanup(), "killed-and-reaped");
        assert!(
            error
                .diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.role() != "fresh-recovery"),
            "recovery must not start after a wrong boundary"
        );
    }

    #[test]
    fn early_exit_before_boundary_reports_role_output_status_and_checkpoint() {
        let root = tempfile::tempdir().expect("state root should exist");
        let error = harness()
            .run(
                root.path(),
                EXPECTED_BOUNDARY,
                EXPECTED_RECOVERY,
                child("early-crash", "early-exit"),
                child("fresh-recovery", "recover"),
            )
            .expect_err("early exit must fail");

        assert_error_report(
            &error,
            &format!("exited before checkpoint \"boundary:{EXPECTED_BOUNDARY}\""),
        );
        let crash = diagnostic(&error, "early-crash");
        assert_eq!(crash.last_checkpoint(), Some("ready"));
        assert_eq!(crash.successful(), Some(false));
        assert!(crash.stderr().contains("before reporting a named boundary"));
    }

    #[test]
    fn boundary_timeout_is_bounded_and_cleanup_reaps_child() {
        let root = tempfile::tempdir().expect("state root should exist");
        let error = harness()
            .run(
                root.path(),
                EXPECTED_BOUNDARY,
                EXPECTED_RECOVERY,
                child("stalled-crash", "stall-before-boundary"),
                child("fresh-recovery", "recover"),
            )
            .expect_err("missing boundary must time out");

        assert_error_report(
            &error,
            &format!("timed out waiting for checkpoint \"boundary:{EXPECTED_BOUNDARY}\""),
        );
        let crash = diagnostic(&error, "stalled-crash");
        assert_eq!(crash.last_checkpoint(), Some("ready"));
        assert_eq!(crash.cleanup(), "killed-and-reaped");
    }

    #[test]
    fn wrong_recovery_observation_keeps_both_phase_diagnostics() {
        let root = tempfile::tempdir().expect("state root should exist");
        let error = harness()
            .run(
                root.path(),
                EXPECTED_BOUNDARY,
                EXPECTED_RECOVERY,
                child("crash-writer", "write-and-boundary"),
                child("wrong-recovery", "wrong-recovery"),
            )
            .expect_err("wrong recovery observation must fail");

        assert_error_report(
            &error,
            &format!(
                "reached checkpoint \"recovered:state-missing:effect-missing\"; expected \"recovered:{EXPECTED_RECOVERY}\""
            ),
        );
        assert_eq!(error.diagnostics().len(), 2);
        assert_eq!(
            diagnostic(&error, "crash-writer").cleanup(),
            "killed-at-boundary-and-reaped"
        );
        assert_eq!(
            diagnostic(&error, "wrong-recovery").last_checkpoint(),
            Some("recovered:state-missing:effect-missing")
        );
    }

    #[test]
    fn recovery_early_exit_and_timeout_are_named_and_cleaned_up() {
        for (mode, expected, cleanup) in [
            (
                "recovery-early-exit",
                "exited before checkpoint",
                "exited-and-reaped",
            ),
            (
                "recovery-stall",
                "timed out waiting for checkpoint",
                "killed-and-reaped",
            ),
        ] {
            let root = tempfile::tempdir().expect("state root should exist");
            let error = harness()
                .run(
                    root.path(),
                    EXPECTED_BOUNDARY,
                    EXPECTED_RECOVERY,
                    child("crash-writer", "write-and-boundary"),
                    child("fresh-recovery", mode),
                )
                .expect_err("recovery failure must fail the harness");

            assert_error_report(&error, expected);
            assert_eq!(
                diagnostic(&error, "fresh-recovery").cleanup(),
                cleanup,
                "mode {mode}"
            );
        }
    }

    #[test]
    #[ignore = "spawned only by the subprocess crash-cut parent tests"]
    fn crash_cut_protocol_child() {
        let mode = std::env::var(MODE_ENV).expect("child test mode should be set");
        match mode.as_str() {
            "write-and-boundary" => run_crash_cut_child(|context| {
                write_synced(context.state_root(), "state", "committed")?;
                write_synced(context.state_root(), "effect", "created")?;
                sync_directory(context.state_root())?;
                context.reach_boundary(EXPECTED_BOUNDARY)
            })
            .unwrap_or_else(|error| panic!("crash child failed: {error}")),
            "wrong-boundary" => {
                run_crash_cut_child(|context| context.reach_boundary("network.store.wrong"))
                    .unwrap_or_else(|error| panic!("wrong-boundary child failed: {error}"))
            }
            "early-exit" => run_crash_cut_child(|_| Ok(()))
                .unwrap_or_else(|error| panic!("intentional early exit: {error}")),
            "stall-before-boundary" => run_crash_cut_child(|_| {
                loop {
                    std::thread::park();
                }
            })
            .unwrap_or_else(|error| panic!("stalled crash child failed: {error}")),
            "recover" => run_crash_recovery_child(recovery_observation)
                .unwrap_or_else(|error| panic!("recovery child failed: {error}")),
            "wrong-recovery" => {
                run_crash_recovery_child(|_| Ok("state-missing:effect-missing".to_owned()))
                    .unwrap_or_else(|error| panic!("wrong recovery child failed: {error}"))
            }
            "recovery-early-exit" => {
                super::emit_checkpoint_for_test_ready()
                    .expect("early recovery should report ready");
                super::expect_inspect_for_test().expect("early recovery should receive inspect");
                panic!("intentional recovery exit before observation");
            }
            "recovery-stall" => {
                super::emit_checkpoint_for_test_ready()
                    .expect("stalled recovery should report ready");
                super::expect_inspect_for_test().expect("stalled recovery should receive inspect");
                loop {
                    std::thread::park();
                }
            }
            other => panic!("unknown crash-harness child mode {other:?}"),
        }
    }

    fn recovery_observation(context: &super::CrashCutChildContext) -> Result<String, String> {
        let state = fs::read_to_string(context.state_root().join("state"))
            .map_err(|error| format!("failed to recover state: {error}"))?;
        let effect = fs::read_to_string(context.state_root().join("effect"))
            .map_err(|error| format!("failed to recover effect: {error}"))?;
        Ok(format!("state-{state}:effect-{effect}"))
    }

    fn write_synced(root: &std::path::Path, name: &str, value: &str) -> Result<(), String> {
        let mut file = File::create(root.join(name))
            .map_err(|error| format!("failed to create {name}: {error}"))?;
        file.write_all(value.as_bytes())
            .map_err(|error| format!("failed to write {name}: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("failed to sync {name}: {error}"))
    }

    #[cfg(unix)]
    fn sync_directory(root: &std::path::Path) -> Result<(), String> {
        File::open(root)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("failed to sync state root: {error}"))
    }

    #[cfg(windows)]
    fn sync_directory(_root: &std::path::Path) -> Result<(), String> {
        Ok(())
    }

    fn child(role: &str, mode: &str) -> ProcessRoleSpec {
        ProcessRoleSpec::new(
            role,
            std::env::current_exe().expect("current test executable should resolve"),
        )
        .arg("--exact")
        .arg(CHILD_TEST)
        .arg("--ignored")
        .arg("--nocapture")
        .env(MODE_ENV, mode)
    }

    fn harness() -> SubprocessCrashCutHarness {
        SubprocessCrashCutHarness::new(Duration::from_secs(2))
    }

    fn diagnostic<'a>(error: &'a ProcessHarnessError, role: &str) -> &'a ProcessDiagnostic {
        error
            .diagnostics()
            .iter()
            .find(|diagnostic| diagnostic.role() == role)
            .unwrap_or_else(|| panic!("missing diagnostic for role {role:?}: {error}"))
    }

    fn assert_error_report(error: &ProcessHarnessError, expected: &str) {
        let rendered = error.to_string();
        assert!(
            rendered.contains(expected),
            "error should contain {expected:?}:\n{rendered}"
        );
        for diagnostic in error.diagnostics() {
            assert!(rendered.contains(&format!("role={:?}", diagnostic.role())));
            assert!(rendered.contains("status="));
            assert!(rendered.contains("last_checkpoint="));
            assert!(rendered.contains("stdout:\n"));
            assert!(rendered.contains("stderr:\n"));
            assert!(!diagnostic.cleanup().is_empty());
        }
    }
}
