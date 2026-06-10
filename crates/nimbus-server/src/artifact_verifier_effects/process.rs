use std::io::{Read, Write};
use std::process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{
    ArtifactVerifierCommandInvocation, ArtifactVerifierCommandOutput,
    ArtifactVerifierCommandRunner, ArtifactVerifierError, ArtifactVerifierResult,
};

#[derive(Debug, Clone, Copy)]
pub struct ProcessArtifactVerifierCommandRunner;

impl ArtifactVerifierCommandRunner for ProcessArtifactVerifierCommandRunner {
    fn run(
        &self,
        invocation: &ArtifactVerifierCommandInvocation,
    ) -> ArtifactVerifierResult<ArtifactVerifierCommandOutput> {
        let mut command = Command::new(invocation.program());
        command
            .args(invocation.args())
            .stdin(if invocation.stdin().is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            ArtifactVerifierError::unavailable(format!(
                "failed to start artifact verifier `{}`: {error}",
                invocation.program()
            ))
        })?;
        let program = invocation.program().to_string();
        let stdin_writer = if let Some(stdin) = invocation.stdin() {
            let child_stdin = child.stdin.take().ok_or_else(|| {
                ArtifactVerifierError::unavailable(format!(
                    "failed to open stdin for artifact verifier `{}`",
                    invocation.program()
                ))
            })?;
            Some(spawn_stdin_writer(
                program.clone(),
                child_stdin,
                stdin.to_string(),
            ))
        } else {
            None
        };
        let stdout_reader = spawn_stdout_reader(
            program.clone(),
            child.stdout.take().ok_or_else(|| {
                ArtifactVerifierError::unavailable(format!(
                    "failed to open stdout for artifact verifier `{}`",
                    invocation.program()
                ))
            })?,
        );
        let stderr_reader = spawn_stderr_reader(
            program.clone(),
            child.stderr.take().ok_or_else(|| {
                ArtifactVerifierError::unavailable(format!(
                    "failed to open stderr for artifact verifier `{}`",
                    invocation.program()
                ))
            })?,
        );
        let deadline = Instant::now() + invocation.timeout();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return collect_child_output(
                        &program,
                        status,
                        stdin_writer,
                        stdout_reader,
                        stderr_reader,
                    );
                }
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    drop(stdin_writer);
                    drop(stdout_reader);
                    drop(stderr_reader);
                    return Err(ArtifactVerifierError::timeout(format!(
                        "artifact verifier `{}` exceeded {}ms",
                        invocation.program(),
                        invocation.timeout().as_millis()
                    )));
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ArtifactVerifierError::unavailable(format!(
                        "failed to observe artifact verifier `{}`: {error}",
                        invocation.program()
                    )));
                }
            }
        }
    }
}

fn spawn_stdin_writer(
    program: String,
    mut child_stdin: ChildStdin,
    stdin: String,
) -> JoinHandle<ArtifactVerifierResult<()>> {
    thread::spawn(move || {
        child_stdin
            .write_all(stdin.as_bytes())
            .map_err(|error| {
                ArtifactVerifierError::unavailable(format!(
                    "failed to write request to artifact verifier `{program}`: {error}"
                ))
            })
            .map(|_| ())
    })
}

fn spawn_stdout_reader(
    program: String,
    child_stdout: ChildStdout,
) -> JoinHandle<ArtifactVerifierResult<String>> {
    spawn_output_reader(program, "stdout", child_stdout)
}

fn spawn_stderr_reader(
    program: String,
    child_stderr: ChildStderr,
) -> JoinHandle<ArtifactVerifierResult<String>> {
    spawn_output_reader(program, "stderr", child_stderr)
}

fn spawn_output_reader<R>(
    program: String,
    stream_name: &'static str,
    mut stream: R,
) -> JoinHandle<ArtifactVerifierResult<String>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut output = Vec::new();
        stream.read_to_end(&mut output).map_err(|error| {
            ArtifactVerifierError::unavailable(format!(
                "failed to read {stream_name} from artifact verifier `{program}`: {error}"
            ))
        })?;
        Ok(String::from_utf8_lossy(&output).into_owned())
    })
}

fn collect_child_output(
    program: &str,
    status: ExitStatus,
    stdin_writer: Option<JoinHandle<ArtifactVerifierResult<()>>>,
    stdout_reader: JoinHandle<ArtifactVerifierResult<String>>,
    stderr_reader: JoinHandle<ArtifactVerifierResult<String>>,
) -> ArtifactVerifierResult<ArtifactVerifierCommandOutput> {
    if let Some(stdin_writer) = stdin_writer {
        join_io_thread(program, "stdin writer", stdin_writer)?;
    }
    let stdout = join_io_thread(program, "stdout reader", stdout_reader)?;
    let stderr = join_io_thread(program, "stderr reader", stderr_reader)?;
    Ok(ArtifactVerifierCommandOutput {
        status_code: status.code(),
        stdout,
        stderr,
    })
}

fn join_io_thread<T>(
    program: &str,
    label: &str,
    handle: JoinHandle<ArtifactVerifierResult<T>>,
) -> ArtifactVerifierResult<T> {
    handle.join().map_err(|_| {
        ArtifactVerifierError::unavailable(format!(
            "artifact verifier `{program}` {label} thread panicked"
        ))
    })?
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        ArtifactVerifierCommandInvocation, ArtifactVerifierCommandRunner,
        ProcessArtifactVerifierCommandRunner,
    };

    #[cfg(unix)]
    #[test]
    fn process_runner_drains_stdout_while_writing_stdin() {
        let invocation = ArtifactVerifierCommandInvocation {
            program: "/bin/sh".to_string(),
            args: vec![
                "-c".to_string(),
                "dd if=/dev/zero bs=1024 count=256 2>/dev/null; cat >/dev/null".to_string(),
            ],
            timeout: Duration::from_secs(5),
            stdin: Some("stdin closes only after stdout is drained".repeat(4096)),
        };

        let output = ProcessArtifactVerifierCommandRunner
            .run(&invocation)
            .expect("runner should drain child stdout while the stdin writer is active");

        assert_eq!(output.status_code, Some(0));
        assert!(
            output.stdout.len() >= 256 * 1024,
            "runner should capture the full large stdout payload, got {} bytes",
            output.stdout.len()
        );
        assert_eq!(output.stderr, "");
    }
}
