use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
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
    // The macOS loader can take several seconds to map the large debug
    // binary on a cold cache. Keep the regression bounded without treating
    // loader startup as a policy failure.
    let output = output_with_timeout(command, Duration::from_secs(30));
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

#[cfg(unix)]
#[test]
fn launcher_gracefully_stops_on_sigterm() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should follow the Unix epoch")
        .as_nanos();
    let test_root =
        std::env::temp_dir().join(format!("nimbus-bin-sigterm-{}-{nonce}", std::process::id()));
    let isolated_home = test_root.join("home");
    let isolated_tmp = test_root.join("tmp");
    std::fs::create_dir_all(&isolated_home).expect("isolated home should exist");
    std::fs::create_dir_all(&isolated_tmp).expect("isolated temporary root should exist");
    let stderr_path = test_root.join("stderr.log");
    let stderr = std::fs::File::create(&stderr_path).expect("stderr log should open");

    let mut command = Command::new(nimbus_bin());
    command
        .args(["start", "--host", "127.0.0.1", "--port", "0", "--data-dir"])
        .arg(test_root.join("data"))
        .arg("--control-data-dir")
        .arg(test_root.join("control"))
        .arg("--network-state-dir")
        .arg(test_root.join("network"))
        .args(["--no-mongodb", "--no-dynamodb", "--no-s3"])
        .env("HOME", &isolated_home)
        .env("TMPDIR", &isolated_tmp)
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr));
    let mut child = command.spawn().expect("nimbus launcher should execute");

    wait_for_log_or_exit(
        &mut child,
        &stderr_path,
        "Nimbus server listening at",
        Duration::from_secs(60),
    );
    // SAFETY: the child PID came from this live `Child`, and SIGTERM is the
    // process contract under test.
    let signal_result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    assert_eq!(signal_result, 0, "SIGTERM should reach the Nimbus child");

    let status = wait_for_status(&mut child, Duration::from_secs(30));
    let stderr = std::fs::read_to_string(&stderr_path).expect("stderr log should be readable");
    let discovery_removed = !isolated_tmp.join("nimbus").join("server.json").exists();
    let _ = std::fs::remove_dir_all(&test_root);
    assert!(
        status.success(),
        "SIGTERM should use the graceful shutdown path: status={status:?} stderr={stderr}"
    );
    assert!(
        discovery_removed,
        "graceful shutdown should remove the live discovery record: {stderr}"
    );
}

#[cfg(unix)]
fn wait_for_log_or_exit(
    child: &mut Child,
    path: &std::path::Path,
    expected: &str,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    loop {
        let log = std::fs::read_to_string(path).unwrap_or_default();
        if log.contains(expected) {
            return;
        }
        if let Some(status) = child
            .try_wait()
            .expect("nimbus launcher status should be readable")
        {
            panic!("nimbus launcher exited before readiness: status={status:?} stderr={log}");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("nimbus launcher did not become ready within {timeout:?}: stderr={log}");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(unix)]
fn wait_for_status(child: &mut Child, timeout: Duration) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .expect("nimbus launcher status should be readable")
        {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let status = child.wait().expect("timed-out Nimbus child should reap");
            panic!("nimbus launcher did not stop within {timeout:?}; forced status={status:?}");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn output_with_timeout(mut command: Command, timeout: Duration) -> Output {
    let program = command.get_program().to_owned();
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
                    "nimbus launcher {} did not exit within {timeout:?}; the public-bind guard may have regressed: stderr={}",
                    program.to_string_lossy(),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}
