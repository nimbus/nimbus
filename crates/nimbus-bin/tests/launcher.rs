use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn nimbus_bin() -> PathBuf {
    std::env::var_os("NEXTEST_BIN_EXE_nimbus")
        .map(PathBuf::from)
        .or_else(|| option_env!("CARGO_BIN_EXE_nimbus").map(PathBuf::from))
        .expect(
            "NEXTEST_BIN_EXE_nimbus should be set by nextest archives, or \
             CARGO_BIN_EXE_nimbus should be set by cargo test",
        )
}

#[test]
fn launcher_runs_nimbus_cli_version_command() {
    let output = Command::new(nimbus_bin())
        .arg("--version")
        .output()
        .expect("nimbus launcher should execute");

    assert!(
        output.status.success(),
        "nimbus --version should succeed: status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("nimbus {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn launcher_renders_cli_errors_for_operators() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should follow the Unix epoch")
        .as_nanos();
    let data_dir = std::env::temp_dir().join(format!(
        "nimbus-bin-launcher-{}-{nonce}",
        std::process::id()
    ));
    let mut command = Command::new(nimbus_bin());
    command.args([
        "start",
        "--host",
        "0.0.0.0",
        "--data-dir",
        data_dir
            .to_str()
            .expect("temporary data path should be valid UTF-8"),
    ]);
    let output = output_with_timeout(command, Duration::from_secs(5));
    let _ = std::fs::remove_dir_all(&data_dir);

    assert!(!output.status.success(), "unsafe public bind should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "error: refusing to bind on non-loopback host `0.0.0.0` without --allow-network."
        ),
        "launcher should render the actionable CLI error: {stderr}"
    );
    assert!(
        !stderr.contains("NonLoopbackRequiresOptIn"),
        "launcher must not expose Rust enum debug text: {stderr}"
    );
}

fn output_with_timeout(mut command: Command, timeout: Duration) -> Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().expect("nimbus launcher should execute");
    let deadline = Instant::now() + timeout;
    loop {
        match child
            .try_wait()
            .expect("nimbus launcher status should be readable")
        {
            Some(_) => {
                return child
                    .wait_with_output()
                    .expect("nimbus launcher output should be readable");
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .expect("timed-out nimbus launcher output should be readable");
                panic!(
                    "nimbus launcher did not exit within {timeout:?}; the public-bind guard may have regressed: stderr={}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}
