use std::path::PathBuf;
use std::process::Command;

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
