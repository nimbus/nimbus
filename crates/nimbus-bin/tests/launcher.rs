use std::process::Command;

#[test]
fn launcher_runs_nimbus_cli_version_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_nimbus"))
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
