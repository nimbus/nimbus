use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
    let output = Command::new(nimbus_bin())
        .args([
            "start",
            "--host",
            "0.0.0.0",
            "--data-dir",
            data_dir
                .to_str()
                .expect("temporary data path should be valid UTF-8"),
        ])
        .output()
        .expect("nimbus launcher should execute");
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
