use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const PROTOCOL_PREFIX: &str = "NIMBUS_PROCESS_HARNESS/1\t";
const ROLE_ENV: &str = "NIMBUS_PROCESS_HARNESS_ROLE";
const STATE_ROOT_ENV: &str = "NIMBUS_PROCESS_HARNESS_STATE_ROOT";

mod crash;

pub use crash::{
    CrashCutChildContext, SubprocessCrashCutHarness, SubprocessCrashCutResult, run_crash_cut_child,
    run_crash_recovery_child,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentionOutcome {
    Won,
    Lost,
}

impl ContentionOutcome {
    fn protocol_value(self) -> &'static str {
        match self {
            Self::Won => "won",
            Self::Lost => "lost",
        }
    }
}

#[derive(Debug)]
pub struct ContentionChildContext {
    role: String,
    state_root: PathBuf,
}

impl ContentionChildContext {
    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn state_root(&self) -> &Path {
        &self.state_root
    }
}

/// Run the child side of the deterministic contention protocol.
///
/// The parent owns ordering. The child acknowledges `ready`, waits for
/// `enter`, acknowledges `entered`, waits for `release`, acknowledges
/// `released`, performs the supplied operation, and reports its terminal
/// contention outcome.
pub fn run_contention_child(
    operation: impl FnOnce(&ContentionChildContext) -> Result<ContentionOutcome, String>,
) -> Result<(), String> {
    let context = child_context_from_env()?;
    emit_checkpoint(Checkpoint::Ready)?;
    expect_command(CommandMessage::Enter)?;
    emit_checkpoint(Checkpoint::Entered)?;
    expect_command(CommandMessage::Release)?;
    emit_checkpoint(Checkpoint::Released)?;
    let outcome = operation(&context)?;
    emit_checkpoint(Checkpoint::Complete(outcome))
}

#[derive(Debug)]
pub struct ProcessRoleSpec {
    role: String,
    program: PathBuf,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
}

impl ProcessRoleSpec {
    pub fn new(role: impl Into<String>, program: impl Into<PathBuf>) -> Self {
        Self {
            role: role.into(),
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    #[must_use]
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwoProcessContentionResult {
    winner: String,
    contender: String,
}

impl TwoProcessContentionResult {
    pub fn winner(&self) -> &str {
        &self.winner
    }

    pub fn contender(&self) -> &str {
        &self.contender
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDiagnostic {
    role: String,
    stdout: String,
    stderr: String,
    status: Option<String>,
    successful: Option<bool>,
    last_checkpoint: Option<String>,
    cleanup: String,
}

impl ProcessDiagnostic {
    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn stdout(&self) -> &str {
        &self.stdout
    }

    pub fn stderr(&self) -> &str {
        &self.stderr
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn successful(&self) -> Option<bool> {
        self.successful
    }

    pub fn last_checkpoint(&self) -> Option<&str> {
        self.last_checkpoint.as_deref()
    }

    pub fn cleanup(&self) -> &str {
        &self.cleanup
    }
}

#[derive(Debug)]
pub struct ProcessHarnessError {
    failure: HarnessFailure,
    diagnostics: Vec<ProcessDiagnostic>,
}

impl ProcessHarnessError {
    pub fn diagnostics(&self) -> &[ProcessDiagnostic] {
        &self.diagnostics
    }
}

impl fmt::Display for ProcessHarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "{}", self.failure)?;
        for diagnostic in &self.diagnostics {
            writeln!(
                formatter,
                "role={:?} status={} last_checkpoint={} cleanup={}",
                diagnostic.role,
                diagnostic
                    .status
                    .as_deref()
                    .unwrap_or("<running-or-unknown>"),
                diagnostic.last_checkpoint.as_deref().unwrap_or("<none>"),
                diagnostic.cleanup
            )?;
            writeln!(formatter, "stdout:\n{}", diagnostic.stdout)?;
            writeln!(formatter, "stderr:\n{}", diagnostic.stderr)?;
        }
        Ok(())
    }
}

impl std::error::Error for ProcessHarnessError {}

#[derive(Debug)]
enum HarnessFailure {
    InvalidConfiguration(String),
    Spawn {
        role: String,
        message: String,
    },
    Protocol {
        role: String,
        message: String,
    },
    UnexpectedCheckpoint {
        role: String,
        expected: String,
        actual: String,
    },
    EarlyExit {
        role: String,
        expected: String,
    },
    Timeout {
        role: String,
        expected: String,
    },
    OutcomeViolation {
        outcomes: Vec<(String, String)>,
    },
}

impl fmt::Display for HarnessFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(
                    formatter,
                    "invalid two-process harness configuration: {message}"
                )
            }
            Self::Spawn { role, message } => {
                write!(formatter, "role {role:?} failed to spawn: {message}")
            }
            Self::Protocol { role, message } => {
                write!(formatter, "role {role:?} protocol failed: {message}")
            }
            Self::UnexpectedCheckpoint {
                role,
                expected,
                actual,
            } => write!(
                formatter,
                "role {role:?} reached checkpoint {actual:?}; expected {expected:?}"
            ),
            Self::EarlyExit { role, expected } => {
                write!(
                    formatter,
                    "role {role:?} exited before checkpoint {expected:?}"
                )
            }
            Self::Timeout { role, expected } => write!(
                formatter,
                "role {role:?} timed out waiting for checkpoint {expected:?}"
            ),
            Self::OutcomeViolation { outcomes } => write!(
                formatter,
                "contention must have exactly one winner and one contender; outcomes={outcomes:?}"
            ),
        }
    }
}

#[derive(Debug)]
pub struct TwoProcessContentionHarness {
    checkpoint_timeout: Duration,
}

impl TwoProcessContentionHarness {
    pub fn new(checkpoint_timeout: Duration) -> Self {
        Self { checkpoint_timeout }
    }

    pub fn run(
        &self,
        state_root: &Path,
        roles: [ProcessRoleSpec; 2],
    ) -> Result<TwoProcessContentionResult, ProcessHarnessError> {
        if self.checkpoint_timeout.is_zero() {
            return Err(ProcessHarnessError {
                failure: HarnessFailure::InvalidConfiguration(
                    "checkpoint timeout must be greater than zero".to_owned(),
                ),
                diagnostics: Vec::new(),
            });
        }
        if roles[0].role.is_empty() || roles[1].role.is_empty() {
            return Err(ProcessHarnessError {
                failure: HarnessFailure::InvalidConfiguration(
                    "both process roles must be non-empty".to_owned(),
                ),
                diagnostics: Vec::new(),
            });
        }
        if roles[0].role == roles[1].role {
            return Err(ProcessHarnessError {
                failure: HarnessFailure::InvalidConfiguration(format!(
                    "process roles must be unique; both were {:?}",
                    roles[0].role
                )),
                diagnostics: Vec::new(),
            });
        }

        fs::create_dir_all(state_root).map_err(|error| ProcessHarnessError {
            failure: HarnessFailure::InvalidConfiguration(format!(
                "state root {} could not be created: {error}",
                state_root.display()
            )),
            diagnostics: Vec::new(),
        })?;
        let state_root = fs::canonicalize(state_root).map_err(|error| ProcessHarnessError {
            failure: HarnessFailure::InvalidConfiguration(format!(
                "state root {} could not be canonicalized: {error}",
                state_root.display()
            )),
            diagnostics: Vec::new(),
        })?;

        let mut children = ChildGroup::default();
        for role in roles {
            match SpawnedRole::spawn(role, &state_root) {
                Ok(child) => children.roles.push(child),
                Err(failure) => {
                    let diagnostics = children.cleanup();
                    return Err(ProcessHarnessError {
                        failure,
                        diagnostics,
                    });
                }
            }
        }

        match children.coordinate(self.checkpoint_timeout) {
            Ok(result) => {
                let diagnostics = children.cleanup();
                let failed_exit = diagnostics
                    .iter()
                    .find(|diagnostic| diagnostic.successful != Some(true));
                if let Some(diagnostic) = failed_exit {
                    return Err(ProcessHarnessError {
                        failure: HarnessFailure::Protocol {
                            role: diagnostic.role.clone(),
                            message: format!(
                                "child acknowledged completion but terminated with {}",
                                diagnostic.status.as_deref().unwrap_or("unknown status")
                            ),
                        },
                        diagnostics,
                    });
                }
                Ok(result)
            }
            Err(failure) => Err(ProcessHarnessError {
                failure,
                diagnostics: children.cleanup(),
            }),
        }
    }
}

#[derive(Default)]
struct ChildGroup {
    roles: Vec<SpawnedRole>,
}

impl ChildGroup {
    fn coordinate(
        &mut self,
        timeout: Duration,
    ) -> Result<TwoProcessContentionResult, HarnessFailure> {
        self.expect_all(CheckpointExpectation::Ready, timeout)?;
        self.send_all(CommandMessage::Enter)?;
        self.expect_all(CheckpointExpectation::Entered, timeout)?;
        self.send_all(CommandMessage::Release)?;
        self.expect_all(CheckpointExpectation::Released, timeout)?;

        let mut outcomes = Vec::with_capacity(self.roles.len());
        let deadline = Instant::now() + timeout;
        for child in &mut self.roles {
            let checkpoint = child.receive_checkpoint(deadline, "complete")?;
            let Checkpoint::Complete(outcome) = checkpoint else {
                return Err(HarnessFailure::UnexpectedCheckpoint {
                    role: child.role.clone(),
                    expected: "complete".to_owned(),
                    actual: checkpoint.to_string(),
                });
            };
            outcomes.push((child.role.clone(), outcome));
        }

        let winners = outcomes
            .iter()
            .filter(|(_, outcome)| *outcome == ContentionOutcome::Won)
            .map(|(role, _)| role.clone())
            .collect::<Vec<_>>();
        let contenders = outcomes
            .iter()
            .filter(|(_, outcome)| *outcome == ContentionOutcome::Lost)
            .map(|(role, _)| role.clone())
            .collect::<Vec<_>>();
        if winners.len() != 1 || contenders.len() != 1 {
            return Err(HarnessFailure::OutcomeViolation {
                outcomes: outcomes
                    .iter()
                    .map(|(role, outcome)| (role.clone(), outcome.protocol_value().to_owned()))
                    .collect(),
            });
        }

        let deadline = Instant::now() + timeout;
        for child in &mut self.roles {
            child.wait_for_stdout_eof(deadline)?;
            child.wait_for_exit()?;
        }

        Ok(TwoProcessContentionResult {
            winner: winners[0].clone(),
            contender: contenders[0].clone(),
        })
    }

    fn expect_all(
        &mut self,
        expectation: CheckpointExpectation,
        timeout: Duration,
    ) -> Result<(), HarnessFailure> {
        let deadline = Instant::now() + timeout;
        for child in &mut self.roles {
            let checkpoint = child.receive_checkpoint(deadline, expectation.as_str())?;
            if !expectation.matches(&checkpoint) {
                return Err(HarnessFailure::UnexpectedCheckpoint {
                    role: child.role.clone(),
                    expected: expectation.as_str().to_owned(),
                    actual: checkpoint.to_string(),
                });
            }
        }
        Ok(())
    }

    fn send_all(&mut self, command: CommandMessage) -> Result<(), HarnessFailure> {
        for child in &mut self.roles {
            child.send_command(command)?;
        }
        Ok(())
    }

    fn cleanup(&mut self) -> Vec<ProcessDiagnostic> {
        self.roles.iter_mut().map(SpawnedRole::cleanup).collect()
    }
}

impl Drop for ChildGroup {
    fn drop(&mut self) {
        for child in &mut self.roles {
            child.ensure_reaped();
        }
    }
}

struct SpawnedRole {
    role: String,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    events: std::sync::mpsc::Receiver<ReaderEvent>,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
    stdout_reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<()>>,
    status: Option<ExitStatus>,
    last_checkpoint: Option<Checkpoint>,
    cleanup_result: Option<String>,
}

impl SpawnedRole {
    fn spawn(spec: ProcessRoleSpec, state_root: &Path) -> Result<Self, HarnessFailure> {
        let mut command = Command::new(&spec.program);
        command
            .args(spec.args)
            .envs(spec.env)
            .env(ROLE_ENV, &spec.role)
            .env(STATE_ROOT_ENV, state_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| HarnessFailure::Spawn {
            role: spec.role.clone(),
            message: format!("{}: {error}", spec.program.display()),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| HarnessFailure::Spawn {
            role: spec.role.clone(),
            message: "spawned child did not expose piped stdin".to_owned(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| HarnessFailure::Spawn {
            role: spec.role.clone(),
            message: "spawned child did not expose piped stdout".to_owned(),
        })?;
        let stderr = child.stderr.take().ok_or_else(|| HarnessFailure::Spawn {
            role: spec.role.clone(),
            message: "spawned child did not expose piped stderr".to_owned(),
        })?;

        let stdout_capture = Arc::new(Mutex::new(String::new()));
        let stderr_capture = Arc::new(Mutex::new(String::new()));
        let (events_sender, events) = std::sync::mpsc::channel();

        let reader_capture = Arc::clone(&stdout_capture);
        let stdout_reader = thread::spawn(move || {
            read_child_stdout(stdout, reader_capture, events_sender);
        });
        let reader_capture = Arc::clone(&stderr_capture);
        let stderr_reader = thread::spawn(move || {
            read_child_stderr(stderr, reader_capture);
        });

        Ok(Self {
            role: spec.role,
            child: Some(child),
            stdin: Some(stdin),
            events,
            stdout: stdout_capture,
            stderr: stderr_capture,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(stderr_reader),
            status: None,
            last_checkpoint: None,
            cleanup_result: None,
        })
    }

    fn receive_checkpoint(
        &mut self,
        deadline: Instant,
        expected: &str,
    ) -> Result<Checkpoint, HarnessFailure> {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| HarnessFailure::Timeout {
                role: self.role.clone(),
                expected: expected.to_owned(),
            })?;
        match self.events.recv_timeout(remaining) {
            Ok(ReaderEvent::Checkpoint(checkpoint)) => {
                self.last_checkpoint = Some(checkpoint.clone());
                Ok(checkpoint)
            }
            Ok(ReaderEvent::Malformed(message)) => Err(HarnessFailure::Protocol {
                role: self.role.clone(),
                message,
            }),
            Ok(ReaderEvent::IoFailure(message)) => Err(HarnessFailure::Protocol {
                role: self.role.clone(),
                message,
            }),
            Ok(ReaderEvent::Eof) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(HarnessFailure::EarlyExit {
                    role: self.role.clone(),
                    expected: expected.to_owned(),
                })
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(HarnessFailure::Timeout {
                role: self.role.clone(),
                expected: expected.to_owned(),
            }),
        }
    }

    fn send_command(&mut self, command: CommandMessage) -> Result<(), HarnessFailure> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(HarnessFailure::Protocol {
                role: self.role.clone(),
                message: format!("stdin closed before command {}", command.as_str()),
            });
        };
        writeln!(stdin, "{PROTOCOL_PREFIX}{}", command.as_str())
            .and_then(|()| stdin.flush())
            .map_err(|error| HarnessFailure::Protocol {
                role: self.role.clone(),
                message: format!("failed to send command {}: {error}", command.as_str()),
            })
    }

    fn wait_for_stdout_eof(&mut self, deadline: Instant) -> Result<(), HarnessFailure> {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| HarnessFailure::Timeout {
                role: self.role.clone(),
                expected: "process exit after complete".to_owned(),
            })?;
        match self.events.recv_timeout(remaining) {
            Ok(ReaderEvent::Eof) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Ok(()),
            Ok(ReaderEvent::Checkpoint(checkpoint)) => {
                self.last_checkpoint = Some(checkpoint.clone());
                Err(HarnessFailure::UnexpectedCheckpoint {
                    role: self.role.clone(),
                    expected: "process exit after complete".to_owned(),
                    actual: checkpoint.to_string(),
                })
            }
            Ok(ReaderEvent::Malformed(message)) | Ok(ReaderEvent::IoFailure(message)) => {
                Err(HarnessFailure::Protocol {
                    role: self.role.clone(),
                    message,
                })
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(HarnessFailure::Timeout {
                role: self.role.clone(),
                expected: "process exit after complete".to_owned(),
            }),
        }
    }

    fn wait_for_exit(&mut self) -> Result<(), HarnessFailure> {
        self.stdin.take();
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        let status = child.wait().map_err(|error| HarnessFailure::Protocol {
            role: self.role.clone(),
            message: format!("failed to wait for child after stdout closed: {error}"),
        })?;
        self.status = Some(status);
        self.cleanup_result = Some("exited-and-reaped".to_owned());
        Ok(())
    }

    fn cleanup(&mut self) -> ProcessDiagnostic {
        self.ensure_reaped();
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
        ProcessDiagnostic {
            role: self.role.clone(),
            stdout: captured(&self.stdout),
            stderr: captured(&self.stderr),
            status: self.status.map(|status| status.to_string()),
            successful: self.status.map(|status| status.success()),
            last_checkpoint: self.last_checkpoint.as_ref().map(ToString::to_string),
            cleanup: self
                .cleanup_result
                .clone()
                .unwrap_or_else(|| "cleanup-state-unknown".to_owned()),
        }
    }

    fn ensure_reaped(&mut self) {
        self.stdin.take();
        let Some(mut child) = self.child.take() else {
            return;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                self.status = Some(status);
                self.cleanup_result = Some("exited-and-reaped".to_owned());
            }
            Ok(None) => {
                let kill_result = child.kill();
                match child.wait() {
                    Ok(status) => {
                        self.status = Some(status);
                        self.cleanup_result = Some(if kill_result.is_ok() {
                            "killed-and-reaped".to_owned()
                        } else {
                            "kill-failed-but-reaped".to_owned()
                        });
                    }
                    Err(error) => {
                        self.cleanup_result = Some(format!(
                            "wait-failed-after-kill={}; kill={}",
                            error,
                            display_io_result(kill_result)
                        ));
                    }
                }
            }
            Err(error) => {
                let kill_result = child.kill();
                let wait_result = child.wait();
                if let Ok(status) = wait_result.as_ref() {
                    self.status = Some(*status);
                }
                self.cleanup_result = Some(format!(
                    "initial-wait-failed={error}; kill={}; final-wait={}",
                    display_io_result(kill_result),
                    display_io_result(wait_result)
                ));
            }
        }
    }
}

#[derive(Debug)]
enum ReaderEvent {
    Checkpoint(Checkpoint),
    Malformed(String),
    IoFailure(String),
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Checkpoint {
    Ready,
    Entered,
    Released,
    Complete(ContentionOutcome),
    Boundary(String),
    Recovered(String),
}

impl Checkpoint {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "ready" => Ok(Self::Ready),
            "entered" => Ok(Self::Entered),
            "released" => Ok(Self::Released),
            "complete:won" => Ok(Self::Complete(ContentionOutcome::Won)),
            "complete:lost" => Ok(Self::Complete(ContentionOutcome::Lost)),
            other if other.starts_with("boundary:") => {
                let boundary = other
                    .strip_prefix("boundary:")
                    .expect("guarded by starts_with");
                validate_semantic_token("boundary", boundary)?;
                Ok(Self::Boundary(boundary.to_owned()))
            }
            other if other.starts_with("recovered:") => {
                let observation = other
                    .strip_prefix("recovered:")
                    .expect("guarded by starts_with");
                validate_semantic_token("recovery observation", observation)?;
                Ok(Self::Recovered(observation.to_owned()))
            }
            other => Err(format!("unrecognized checkpoint {other:?}")),
        }
    }
}

impl fmt::Display for Checkpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready => formatter.write_str("ready"),
            Self::Entered => formatter.write_str("entered"),
            Self::Released => formatter.write_str("released"),
            Self::Complete(ContentionOutcome::Won) => formatter.write_str("complete:won"),
            Self::Complete(ContentionOutcome::Lost) => formatter.write_str("complete:lost"),
            Self::Boundary(boundary) => write!(formatter, "boundary:{boundary}"),
            Self::Recovered(observation) => write!(formatter, "recovered:{observation}"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum CheckpointExpectation {
    Ready,
    Entered,
    Released,
}

impl CheckpointExpectation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Entered => "entered",
            Self::Released => "released",
        }
    }

    fn matches(self, checkpoint: &Checkpoint) -> bool {
        matches!(
            (self, checkpoint),
            (Self::Ready, &Checkpoint::Ready)
                | (Self::Entered, &Checkpoint::Entered)
                | (Self::Released, &Checkpoint::Released)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandMessage {
    Enter,
    Release,
    Run,
    Inspect,
}

impl CommandMessage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Enter => "enter",
            Self::Release => "release",
            Self::Run => "run",
            Self::Inspect => "inspect",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "enter" => Ok(Self::Enter),
            "release" => Ok(Self::Release),
            "run" => Ok(Self::Run),
            "inspect" => Ok(Self::Inspect),
            other => Err(format!("unrecognized parent command {other:?}")),
        }
    }
}

fn validate_semantic_token(kind: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{kind} must not be empty"));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
    {
        return Err(format!(
            "{kind} {value:?} contains characters outside [A-Za-z0-9._:-]"
        ));
    }
    Ok(())
}

fn child_context_from_env() -> Result<ContentionChildContext, String> {
    let role = std::env::var(ROLE_ENV)
        .map_err(|error| format!("missing or invalid child role in {ROLE_ENV}: {error}"))?;
    let state_root = std::env::var_os(STATE_ROOT_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing child state root in {STATE_ROOT_ENV}"))?;
    Ok(ContentionChildContext { role, state_root })
}

fn expect_command(expected: CommandMessage) -> Result<(), String> {
    let mut line = String::new();
    let read = io::stdin()
        .read_line(&mut line)
        .map_err(|error| format!("failed to read parent command: {error}"))?;
    if read == 0 {
        return Err(format!(
            "parent stdin closed before command {:?}",
            expected.as_str()
        ));
    }
    let value = line
        .trim_end()
        .strip_prefix(PROTOCOL_PREFIX)
        .ok_or_else(|| format!("parent command lacked protocol prefix: {line:?}"))?;
    let actual = CommandMessage::parse(value)?;
    if actual != expected {
        return Err(format!(
            "received parent command {:?}; expected {:?}",
            actual.as_str(),
            expected.as_str()
        ));
    }
    Ok(())
}

fn emit_checkpoint(checkpoint: Checkpoint) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{PROTOCOL_PREFIX}{checkpoint}")
        .and_then(|()| stdout.flush())
        .map_err(|error| format!("failed to emit checkpoint {checkpoint}: {error}"))
}

fn read_child_stdout(
    stdout: impl Read,
    capture: Arc<Mutex<String>>,
    events: std::sync::mpsc::Sender<ReaderEvent>,
) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                let _ = events.send(ReaderEvent::Eof);
                return;
            }
            Ok(_) => {
                append_capture(&capture, &line);
                if let Some(value) = line.trim_end().strip_prefix(PROTOCOL_PREFIX) {
                    match Checkpoint::parse(value) {
                        Ok(checkpoint) => {
                            if events.send(ReaderEvent::Checkpoint(checkpoint)).is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            if events.send(ReaderEvent::Malformed(error)).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
            Err(error) => {
                let _ = events.send(ReaderEvent::IoFailure(format!(
                    "failed to read child stdout: {error}"
                )));
                return;
            }
        }
    }
}

fn read_child_stderr(stderr: impl Read, capture: Arc<Mutex<String>>) {
    let mut reader = BufReader::new(stderr);
    let mut buffer = String::new();
    match reader.read_to_string(&mut buffer) {
        Ok(_) => append_capture(&capture, &buffer),
        Err(error) => append_capture(
            &capture,
            &format!("\n<failed to read child stderr: {error}>\n"),
        ),
    }
}

fn append_capture(capture: &Mutex<String>, value: &str) {
    capture
        .lock()
        .expect("process harness capture lock should not be poisoned")
        .push_str(value);
}

fn captured(capture: &Mutex<String>) -> String {
    capture
        .lock()
        .expect("process harness capture lock should not be poisoned")
        .clone()
}

fn display_io_result<T: fmt::Debug>(result: io::Result<T>) -> String {
    match result {
        Ok(value) => format!("ok({value:?})"),
        Err(error) => format!("error({error})"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Checkpoint, ContentionOutcome, ProcessHarnessError, ProcessRoleSpec,
        TwoProcessContentionHarness, emit_checkpoint, run_contention_child,
    };
    use std::fs::{self, OpenOptions};
    use std::io::{self, Read, Write};
    use std::time::Duration;

    const CHILD_TEST: &str = "process_harness::tests::contention_protocol_child";
    const MODE_ENV: &str = "NIMBUS_PROCESS_HARNESS_TEST_MODE";

    #[test]
    fn two_real_children_contend_over_one_state_root_with_one_winner() {
        let root = tempfile::tempdir().expect("state root should exist");
        let result = harness(Duration::from_secs(5))
            .run(
                root.path(),
                [child("alpha", "contend"), child("beta", "contend")],
            )
            .expect("contention protocol should complete");

        assert_ne!(result.winner(), result.contender());
        let recorded_winner =
            fs::read_to_string(root.path().join("winner")).expect("winner should be durable");
        assert_eq!(recorded_winner, result.winner());
    }

    #[test]
    fn missing_participant_reports_both_roles_and_no_checkpoint() {
        let root = tempfile::tempdir().expect("state root should exist");
        let error = harness(Duration::from_secs(2))
            .run(
                root.path(),
                [child("alpha", "contend"), child("missing", "silent")],
            )
            .expect_err("silent participant must time out");

        assert_error_report(&error, "timed out waiting for checkpoint \"ready\"");
        let missing = diagnostic(&error, "missing");
        assert_eq!(missing.last_checkpoint(), None);
        assert!(missing.stdout().contains("running 1 test"));
    }

    #[test]
    fn wrong_checkpoint_reports_expected_actual_and_last_checkpoint() {
        let root = tempfile::tempdir().expect("state root should exist");
        let error = harness(Duration::from_secs(2))
            .run(
                root.path(),
                [child("wrong", "wrong-checkpoint"), child("beta", "contend")],
            )
            .expect_err("out-of-order completion must fail");

        assert_error_report(
            &error,
            "reached checkpoint \"complete:won\"; expected \"ready\"",
        );
        assert_eq!(
            diagnostic(&error, "wrong").last_checkpoint(),
            Some("complete:won")
        );
    }

    #[test]
    fn early_exit_reports_status_stderr_and_last_checkpoint() {
        let root = tempfile::tempdir().expect("state root should exist");
        let error = harness(Duration::from_secs(2))
            .run(
                root.path(),
                [child("early", "early-exit"), child("beta", "contend")],
            )
            .expect_err("early child exit must fail");

        assert_error_report(&error, "exited before checkpoint \"ready\"");
        let early = diagnostic(&error, "early");
        assert_eq!(early.successful(), Some(true));
        assert!(early.status().is_some());
        assert_eq!(early.last_checkpoint(), None);
        assert!(early.stderr().contains("intentional early exit"));
    }

    #[test]
    fn checkpoint_timeout_reports_released_boundary_without_sleeping() {
        let root = tempfile::tempdir().expect("state root should exist");
        let error = harness(Duration::from_secs(2))
            .run(
                root.path(),
                [
                    child("stalled", "stall-after-release"),
                    child("beta", "contend"),
                ],
            )
            .expect_err("missing completion must time out");

        assert_error_report(&error, "timed out waiting for checkpoint \"complete\"");
        assert_eq!(
            diagnostic(&error, "stalled").last_checkpoint(),
            Some("released")
        );
    }

    #[test]
    fn failure_cleanup_kills_and_reaps_every_running_child() {
        let root = tempfile::tempdir().expect("state root should exist");
        let error = harness(Duration::from_secs(2))
            .run(
                root.path(),
                [child("alpha", "silent"), child("beta", "silent")],
            )
            .expect_err("silent children must be cleaned up");

        assert_eq!(error.diagnostics().len(), 2);
        for child in error.diagnostics() {
            assert_eq!(child.cleanup(), "killed-and-reaped");
            assert!(
                child.status().is_some(),
                "{} should retain its reaped status",
                child.role()
            );
        }
    }

    #[test]
    #[ignore = "spawned only by the process-harness parent tests"]
    fn contention_protocol_child() {
        let mode = std::env::var(MODE_ENV).expect("child test mode should be set");
        match mode.as_str() {
            "contend" => run_contention_child(contend)
                .unwrap_or_else(|error| panic!("child contention protocol failed: {error}")),
            "silent" => {
                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .expect("silent child should wait on parent stdin");
                loop {
                    std::thread::park();
                }
            }
            "wrong-checkpoint" => {
                emit_checkpoint(Checkpoint::Complete(ContentionOutcome::Won))
                    .expect("wrong checkpoint should be emitted");
            }
            "early-exit" => {
                eprintln!("intentional early exit before ready");
            }
            "stall-after-release" => run_contention_child(|_| {
                let mut input = [0_u8; 1];
                io::stdin()
                    .read_exact(&mut input)
                    .expect("stall child should remain blocked until parent cleanup");
                Err("parent unexpectedly wrote after release".to_owned())
            })
            .unwrap_or_else(|error| panic!("stall child protocol failed: {error}")),
            other => panic!("unknown child test mode {other:?}"),
        }
    }

    fn contend(context: &super::ContentionChildContext) -> Result<ContentionOutcome, String> {
        let winner = context.state_root().join("winner");
        match OpenOptions::new().write(true).create_new(true).open(winner) {
            Ok(mut file) => {
                file.write_all(context.role().as_bytes())
                    .map_err(|error| format!("failed to record winning role: {error}"))?;
                file.sync_all()
                    .map_err(|error| format!("failed to sync winning role: {error}"))?;
                Ok(ContentionOutcome::Won)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                Ok(ContentionOutcome::Lost)
            }
            Err(error) => Err(format!("failed to contend for winner file: {error}")),
        }
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

    fn harness(timeout: Duration) -> TwoProcessContentionHarness {
        TwoProcessContentionHarness::new(timeout)
    }

    fn diagnostic<'a>(error: &'a ProcessHarnessError, role: &str) -> &'a super::ProcessDiagnostic {
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
        for role in ["alpha", "beta", "missing", "wrong", "early", "stalled"] {
            if let Some(diagnostic) = error
                .diagnostics()
                .iter()
                .find(|diagnostic| diagnostic.role() == role)
            {
                assert!(rendered.contains(&format!("role={role:?}")));
                assert!(rendered.contains("status="));
                assert!(rendered.contains("last_checkpoint="));
                assert!(rendered.contains("stdout:\n"));
                assert!(rendered.contains("stderr:\n"));
                assert!(!diagnostic.cleanup().is_empty());
            }
        }
    }

    #[test]
    fn invalid_zero_timeout_fails_before_spawning() {
        let root = tempfile::tempdir().expect("state root should exist");
        let error = harness(Duration::ZERO)
            .run(
                root.path(),
                [child("alpha", "contend"), child("beta", "contend")],
            )
            .expect_err("zero timeout should be rejected");

        assert!(
            error
                .to_string()
                .contains("checkpoint timeout must be greater than zero")
        );
        assert!(error.diagnostics().is_empty());
        assert!(!root.path().join("winner").exists());
    }
}
