use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::*;

fn child_fixture() -> CompiledWorkloadNetworkPlan {
    let decision = standalone_decision(
        TENANT,
        "python",
        "sandbox-a",
        SandboxBackendKind::Container,
        Some(GENERATION),
        Some("node-a"),
    );
    let spec = sandbox_spec(
        TENANT,
        SandboxOwnerSpec::standalone(),
        SandboxBackendKind::Container,
        [SandboxPortBinding::new(
            "http",
            EndpointProtocol::Http,
            18080,
            8080,
        )],
    );
    let (registry, selection) = registry_for(SandboxBackendKind::Container, false);
    compile_standalone(&decision, &spec, &selection, &registry)
        .expect("child fixture should compile")
}

#[test]
#[ignore = "spawned only by the NNC6.2 cross-process determinism parent"]
fn compiler_child_payload() {
    assert!(
        std::env::var_os("NIMBUS_NNC62_CHILD").is_some(),
        "the child-only compiler payload requires its explicit process boundary marker"
    );
    println!(
        "NNC62_CHILD:{}",
        serde_json::to_string(&child_fixture()).expect("compiled payload should serialize")
    );
}

#[test]
fn compiler_is_byte_deterministic_in_a_distinct_process() {
    let expected = serde_json::to_string(&child_fixture()).expect("compiled payload should encode");
    let mut child = Command::new(std::env::current_exe().expect("test executable should exist"))
        .env("NIMBUS_NNC62_CHILD", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args([
            "--ignored",
            "--exact",
            "workload_network_plan::tests::child_process::compiler_child_payload",
            "--nocapture",
        ])
        .spawn()
        .expect("child test process should start");

    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "distinct-process compiler proof exceeded 15 seconds before producing the NNC62_CHILD payload"
                );
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("failed to inspect distinct-process compiler child: {error}");
            }
        }
    };

    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("child stdout should be piped")
        .read_to_string(&mut stdout)
        .expect("child stdout should be UTF-8");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("child stderr should be piped")
        .read_to_string(&mut stderr)
        .expect("child stderr should be UTF-8");
    assert!(
        status.success(),
        "child compiler failed with {status}: {stderr}"
    );
    let actual = stdout
        .lines()
        .find_map(|line| line.strip_prefix("NNC62_CHILD:"))
        .expect("child payload marker should exist");
    assert_eq!(actual, expected);
}
