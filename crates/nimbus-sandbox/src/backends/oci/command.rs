use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use serde::{Deserialize, Serialize};

const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(5);
const MAX_CAPTURED_STREAM_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn as_command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command
    }
}

pub(crate) fn render_command_failure(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_owned();
    if !stderr.is_empty() {
        return stderr;
    }

    let stdout = String::from_utf8_lossy(stdout).trim().to_owned();
    if stdout.is_empty() {
        "stdout and stderr were empty".to_owned()
    } else {
        stdout
    }
}

/// Spawn one provider query with bounded completion and bounded diagnostics.
///
/// Anonymous regular files capture both streams without pipe backpressure or
/// joinable reader threads. Observation retains at most
/// `MAX_CAPTURED_STREAM_BYTES` per stream. The owner kills and reaps on
/// timeout; every other post-spawn error either retains that owner through
/// cleanup or transfers it to a dedicated reaper before returning.
pub(crate) fn run_bounded_command_output(
    command: &mut Command,
    timeout: Duration,
) -> std::io::Result<Output> {
    run_bounded_command_output_with_termination(command, timeout, OwnedCommandChild::terminate)
}

fn run_bounded_command_output_with_termination(
    command: &mut Command,
    timeout: Duration,
    mut terminate: impl FnMut(&mut OwnedCommandChild) -> std::io::Result<()>,
) -> std::io::Result<Output> {
    // Regular anonymous files keep observation bounded even if a provider
    // daemonizes a descendant that retains the inherited output descriptors.
    // Pipes would require reader threads whose joins can be held forever by
    // such a descendant.
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?));
    #[cfg(unix)]
    {
        command.process_group(0);
        // SAFETY: the closure calls only async-signal-safe `setrlimit` and
        // constructs an OS error. It runs after fork and before exec.
        unsafe {
            command.pre_exec(|| {
                let capture_limit = libc::rlimit {
                    rlim_cur: MAX_CAPTURED_STREAM_BYTES as libc::rlim_t,
                    rlim_max: MAX_CAPTURED_STREAM_BYTES as libc::rlim_t,
                };
                if libc::setrlimit(libc::RLIMIT_FSIZE, &capture_limit) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }
    let child = command.spawn()?;
    let mut owned = OwnedCommandChild::new(child);
    #[cfg(unix)]
    {
        let process_group = i32::try_from(owned.child_id()).map_err(|_| {
            std::io::Error::other(format!(
                "provider command PID {} cannot identify its process group",
                owned.child_id()
            ))
        })?;
        owned.set_process_group(process_group);
    }
    wait_for_bounded_file_output(
        &mut owned,
        timeout,
        &mut stdout,
        &mut stderr,
        &mut terminate,
    )
}

fn wait_for_bounded_file_output(
    owned: &mut OwnedCommandChild,
    timeout: Duration,
    stdout: &mut File,
    stderr: &mut File,
    terminate: &mut impl FnMut(&mut OwnedCommandChild) -> std::io::Result<()>,
) -> std::io::Result<Output> {
    let deadline = Instant::now() + timeout;
    loop {
        match owned.try_wait()? {
            Some(status) => {
                // `try_wait` has reaped the leader, so its numeric process
                // group must never be signaled again: that PGID can now be
                // recycled. Regular-file captures make completion independent
                // of descendants retaining output descriptors, and the
                // inherited RLIMIT_FSIZE bounds every such writer.
                owned.disarm_after_reap();
                let (stdout, stderr) = read_bounded_captures(stdout, stderr)?;
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            None if Instant::now() < deadline => {
                std::thread::sleep(COMMAND_POLL_INTERVAL.min(timeout));
            }
            None => {
                if let Err(error) = terminate(owned) {
                    let kind = error.kind();
                    return Err(std::io::Error::new(
                        kind,
                        format!(
                            "provider command termination failed; cleanup retained by owned \
                             reaper: {error}"
                        ),
                    ));
                }
                let _ = owned.wait_after_termination()?;
                // Read both bounded captures before reporting the timeout so
                // capture failures cannot strand unread provider evidence.
                let _ = read_bounded_captures(stdout, stderr)?;
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("provider command exceeded {timeout:?}"),
                ));
            }
        }
    }
}

fn read_bounded_captures(
    stdout: &mut File,
    stderr: &mut File,
) -> std::io::Result<(Vec<u8>, Vec<u8>)> {
    let stdout_result = read_bounded_capture(stdout);
    let stderr_result = read_bounded_capture(stderr);
    Ok((stdout_result?, stderr_result?))
}

fn read_bounded_capture(file: &mut File) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0))?;
    let mut retained = Vec::new();
    file.take(MAX_CAPTURED_STREAM_BYTES as u64)
        .read_to_end(&mut retained)?;
    Ok(retained)
}

struct OwnedCommandChild {
    child: Option<Child>,
    #[cfg(unix)]
    process_group: Option<libc::pid_t>,
}

impl OwnedCommandChild {
    fn new(child: Child) -> Self {
        Self {
            child: Some(child),
            #[cfg(unix)]
            process_group: None,
        }
    }

    fn child_id(&self) -> u32 {
        self.child
            .as_ref()
            .expect("owned provider child must exist before it is reaped")
            .id()
    }

    #[cfg(unix)]
    fn set_process_group(&mut self, process_group: libc::pid_t) {
        self.process_group = Some(process_group);
    }

    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child
            .as_mut()
            .expect("owned provider child must exist while polling")
            .try_wait()
    }

    fn terminate(&mut self) -> std::io::Result<()> {
        #[cfg(unix)]
        if let Some(process_group) = self.process_group {
            match signal_process_group(process_group, libc::SIGKILL) {
                Ok(()) => {
                    self.process_group = None;
                    return Ok(());
                }
                Err(error) if error.raw_os_error() == Some(libc::ESRCH) => {
                    self.process_group = None;
                }
                Err(error) => return Err(error),
            }
        }
        match self
            .child
            .as_mut()
            .expect("owned provider child must exist before termination")
            .kill()
        {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn wait_after_termination(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let status = self
            .child
            .as_mut()
            .expect("owned provider child must exist before reap")
            .wait()?;
        self.child = None;
        #[cfg(unix)]
        {
            self.process_group = None;
        }
        Ok(status)
    }

    fn disarm_after_reap(&mut self) {
        self.child = None;
        #[cfg(unix)]
        {
            self.process_group = None;
        }
    }
}

impl Drop for OwnedCommandChild {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(process_group) = self.process_group.take() {
            let _ = signal_process_group(process_group, libc::SIGKILL);
        }
        if let Some(child) = self.child.take() {
            transfer_child_reap(child);
        }
    }
}

fn transfer_child_reap(child: Child) {
    let (sender, receiver) = std::sync::mpsc::channel::<Child>();
    match std::thread::Builder::new()
        .name("nimbus-provider-observation-reaper".into())
        .spawn(move || {
            let Ok(mut child) = receiver.recv() else {
                return;
            };
            let _ = child.kill();
            let _ = child.wait();
        }) {
        Ok(_worker) => {
            if let Err(send_error) = sender.send(child) {
                let mut child = send_error.0;
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        Err(_) => {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(unix)]
fn signal_process_group(process_group: libc::pid_t, signal: libc::c_int) -> std::io::Result<()> {
    // SAFETY: `process_group` is the positive PID captured immediately after
    // spawning a child with `process_group(0)`. A negative pid targets that
    // exact process group and no unrelated process.
    let result = unsafe { libc::kill(-process_group, signal) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_command_failure_prefers_stderr() {
        assert_eq!(
            render_command_failure(b"stdout detail", b"stderr detail"),
            "stderr detail"
        );
    }

    #[test]
    fn render_command_failure_falls_back_to_stdout_then_empty_message() {
        assert_eq!(
            render_command_failure(b"stdout detail", b""),
            "stdout detail"
        );
        assert_eq!(
            render_command_failure(b"", b""),
            "stdout and stderr were empty"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_command_kills_and_reaps_a_timed_out_child() {
        let pidfile = tempfile::NamedTempFile::new().expect("descendant pidfile should exist");
        let pidfile_path = pidfile.path().to_owned();
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            &format!(
                "sleep 5 & printf '%s' \"$!\" > '{}'; wait",
                pidfile_path.display()
            ),
        ]);
        let started = Instant::now();
        let error = run_bounded_command_output(&mut command, Duration::from_millis(20))
            .expect_err("sleep must exceed the command deadline");

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "deadline enforcement must not wait for the original child duration"
        );
        let descendant_pid = std::fs::read_to_string(&pidfile_path)
            .expect("descendant pid should be captured")
            .parse::<libc::pid_t>()
            .expect("descendant pid should parse");
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            // SAFETY: signal zero observes only the captured provider
            // descendant and never changes its state.
            let result = unsafe { libc::kill(descendant_pid, 0) };
            if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "provider descendant {descendant_pid} must be terminated by timeout ownership"
            );
            std::thread::sleep(COMMAND_POLL_INTERVAL);
        }
    }

    #[cfg(unix)]
    #[test]
    fn bounded_command_retains_reap_ownership_when_termination_fails() {
        let mut command = Command::new("sleep");
        command.arg("0.05");
        let pid = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
        let captured_pid = pid.clone();
        let error = run_bounded_command_output_with_termination(
            &mut command,
            Duration::from_millis(1),
            move |owned| {
                captured_pid.store(
                    owned.child_id() as libc::pid_t,
                    std::sync::atomic::Ordering::SeqCst,
                );
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected termination failure",
                ))
            },
        )
        .expect_err("injected termination failure must remain an error");

        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        let pid = pid.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            pid > 0,
            "the injected terminator must observe the owned child"
        );
        std::thread::sleep(Duration::from_millis(200));
        let status = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
        assert_eq!(
            status, -1,
            "the timeout owner must transfer the child to a reaper"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD),
            "the child must already be reaped rather than remain a zombie"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_command_caps_the_actual_provider_capture_file() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "yes x | head -c 200000"]);
        let output = run_bounded_command_output(&mut command, Duration::from_secs(2))
            .expect("capture-limit termination should remain a completed observation");

        assert!(
            !output.status.success(),
            "a provider that exceeds the on-disk capture limit must fail closed"
        );
        assert_eq!(output.stdout.len(), MAX_CAPTURED_STREAM_BYTES);
        assert!(
            output.stderr.len() <= MAX_CAPTURED_STREAM_BYTES,
            "provider diagnostics must remain bounded even when the shell reports the file-size signal"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_command_does_not_wait_for_an_escaped_session_holding_output_descriptors() {
        let executable = std::env::current_exe().expect("current test executable should resolve");
        let mut command = Command::new(executable);
        command
            .args([
                "--exact",
                "backends::oci::command::tests::escaped_session_provider_helper",
                "--nocapture",
            ])
            .env("NIMBUS_ESCAPED_PROVIDER_HELPER", "1");
        let started = Instant::now();

        let output = run_bounded_command_output(&mut command, Duration::from_millis(500))
            .expect("an escaped descriptor holder must not retain observation ownership");

        assert!(
            output.status.success(),
            "the direct provider leader should report successful handoff"
        );
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "anonymous capture files must make observation independent of an escaped descriptor holder"
        );
    }

    #[cfg(unix)]
    #[test]
    fn escaped_session_provider_helper() {
        if std::env::var_os("NIMBUS_ESCAPED_PROVIDER_HELPER").is_none() {
            return;
        }

        // SAFETY: both post-fork branches call only async-signal-safe libc
        // functions before `_exit`. The parent exits immediately; the child
        // creates a new session and attempts to grow the inherited capture.
        // The inherited RLIMIT_FSIZE terminates it at the exact bound.
        unsafe {
            let pid = libc::fork();
            if pid < 0 {
                libc::_exit(2);
            }
            if pid > 0 {
                libc::_exit(0);
            }
            if libc::setsid() < 0 {
                libc::_exit(3);
            }
            let chunk = [b'x'; 4096];
            loop {
                if libc::write(libc::STDOUT_FILENO, chunk.as_ptr().cast(), chunk.len()) <= 0 {
                    break;
                }
            }
            libc::_exit(0);
        }
    }
}
