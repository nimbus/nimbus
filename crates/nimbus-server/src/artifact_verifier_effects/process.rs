use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
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
        if let Some(stdin) = invocation.stdin() {
            let mut child_stdin = child.stdin.take().ok_or_else(|| {
                ArtifactVerifierError::unavailable(format!(
                    "failed to open stdin for artifact verifier `{}`",
                    invocation.program()
                ))
            })?;
            child_stdin.write_all(stdin.as_bytes()).map_err(|error| {
                ArtifactVerifierError::unavailable(format!(
                    "failed to write request to artifact verifier `{}`: {error}",
                    invocation.program()
                ))
            })?;
        }
        let deadline = Instant::now() + invocation.timeout();
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    let output = child.wait_with_output().map_err(|error| {
                        ArtifactVerifierError::unavailable(format!(
                            "failed to collect artifact verifier `{}` output: {error}",
                            invocation.program()
                        ))
                    })?;
                    return Ok(ArtifactVerifierCommandOutput {
                        status_code: output.status.code(),
                        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    });
                }
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
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
