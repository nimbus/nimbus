use std::path::Path;
use std::process::{Child, Command, Stdio};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

use tempfile::TempDir;

use super::*;

const RUNTIME_ID: &str = "runtime-process-test";
const ATTEMPT_ID: &str = "creator-attempt-test";

struct SentinelProcess {
    child: Child,
}

impl SentinelProcess {
    fn spawn() -> Self {
        let child = Command::new("/bin/sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("sentinel process should spawn");
        Self { child }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn assert_alive(&mut self) {
        assert!(
            self.child
                .try_wait()
                .expect("sentinel process should be observable")
                .is_none(),
            "rejected evidence must not signal the sentinel process"
        );
    }

    #[cfg(target_os = "linux")]
    fn wait_for_exit(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if self
                .child
                .try_wait()
                .expect("signalled sentinel should be observable")
                .is_some()
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "authenticated signal did not terminate the sentinel before the deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for SentinelProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn state_command(runtime_id: &str, attempt_id: &str, pid: Option<u32>) -> CommandSpec {
    let payload = serde_json::json!({
        "id": runtime_id,
        "status": "running",
        "pid": pid,
        "annotations": {
            "com.nimbus.creator-attempt": attempt_id,
        },
    });
    CommandSpec::new("/bin/echo").arg(payload.to_string())
}

fn fixture() -> (TempDir, SentinelProcess, CommandSpec) {
    let root = TempDir::new().expect("temporary process fixture should exist");
    let sentinel = SentinelProcess::spawn();
    std::fs::write(root.path().join("runtime.pid"), sentinel.pid().to_string())
        .expect("runtime pidfile should be written");
    let state = state_command(RUNTIME_ID, ATTEMPT_ID, Some(sentinel.pid()));
    (root, sentinel, state)
}

fn capture_fixture_identity(
    root: &TempDir,
    sentinel: &SentinelProcess,
    state: &CommandSpec,
) -> RuntimeProcessIdentity {
    let identity = capture_runtime_process_identity(
        state,
        RUNTIME_ID,
        ATTEMPT_ID,
        &root.path().join("runtime.pid"),
    )
    .expect("mutually consistent process evidence should capture");
    assert_eq!(identity.pid(), sentinel.pid());
    identity
}

fn assert_rejected_without_signal(
    identity: &RuntimeProcessIdentity,
    state: &CommandSpec,
    pidfile: &Path,
    sentinel: &mut SentinelProcess,
) {
    assert!(
        inspect_runtime_process_identity(identity, state, pidfile).is_err(),
        "mismatched evidence must fail closed"
    );
    #[cfg(target_os = "linux")]
    assert!(
        signal_authenticated_runtime_process(
            identity,
            state,
            pidfile,
            RuntimeProcessSignal::kill()
        )
        .is_err(),
        "mismatched evidence must fail before pidfd_send_signal"
    );
    sentinel.assert_alive();
}

#[test]
fn exact_runtime_process_identity_captures_all_stable_evidence() {
    let (root, mut sentinel, state) = fixture();
    let identity = capture_fixture_identity(&root, &sentinel, &state);

    assert_eq!(identity.runtime_id(), RUNTIME_ID);
    assert_eq!(identity.creator_attempt_id(), ATTEMPT_ID);
    assert_eq!(identity.pid(), sentinel.pid());
    assert_eq!(
        inspect_runtime_process_identity(&identity, &state, &root.path().join("runtime.pid"))
            .expect("captured process should inspect"),
        RuntimeProcessIdentityObservation::ExactLive
    );

    let value = serde_json::to_value(&identity).expect("identity should serialize");
    assert!(value.get("runtimeId").is_some());
    assert!(value.get("creatorAttemptId").is_some());
    assert!(value.get("pid").is_some());
    assert!(value.get("birth").is_some());
    assert!(value.get("fd").is_none());
    assert!(value.get("pidfd").is_none());
    assert_eq!(
        serde_json::from_value::<RuntimeProcessIdentity>(value)
            .expect("durable identity should deserialize"),
        identity
    );
    sentinel.assert_alive();
}

#[test]
fn every_crossed_or_missing_identity_fails_before_signal() {
    let (root, mut sentinel, exact_state) = fixture();
    let pidfile = root.path().join("runtime.pid");
    let identity = capture_fixture_identity(&root, &sentinel, &exact_state);

    let foreign_runtime = state_command("foreign-runtime", ATTEMPT_ID, Some(sentinel.pid()));
    assert_rejected_without_signal(&identity, &foreign_runtime, &pidfile, &mut sentinel);

    let foreign_attempt = state_command(RUNTIME_ID, "foreign-attempt", Some(sentinel.pid()));
    assert_rejected_without_signal(&identity, &foreign_attempt, &pidfile, &mut sentinel);

    let crossed_provider_pid = state_command(
        RUNTIME_ID,
        ATTEMPT_ID,
        Some(sentinel.pid().saturating_add(1)),
    );
    assert_rejected_without_signal(&identity, &crossed_provider_pid, &pidfile, &mut sentinel);

    let missing_provider_pid = state_command(RUNTIME_ID, ATTEMPT_ID, None);
    assert_rejected_without_signal(&identity, &missing_provider_pid, &pidfile, &mut sentinel);

    std::fs::write(&pidfile, sentinel.pid().saturating_add(1).to_string())
        .expect("crossed pidfile should be written");
    assert_rejected_without_signal(&identity, &exact_state, &pidfile, &mut sentinel);

    std::fs::remove_file(&pidfile).expect("pidfile should be removed");
    assert_rejected_without_signal(&identity, &exact_state, &pidfile, &mut sentinel);

    std::fs::write(&pidfile, sentinel.pid().to_string()).expect("exact pidfile should be restored");
    let recycled_identity = identity.with_substituted_birth_for_test();
    assert_rejected_without_signal(&recycled_identity, &exact_state, &pidfile, &mut sentinel);
}

#[test]
fn reserved_pid_zero_and_non_regular_pidfiles_fail_closed() {
    let root = TempDir::new().expect("temporary process fixture should exist");
    let pidfile = root.path().join("runtime.pid");
    std::fs::write(&pidfile, "0\n").expect("zero pidfile should be written");
    assert!(read_regular_pidfile(&pidfile).is_err());

    std::fs::remove_file(&pidfile).expect("zero pidfile should be removed");
    std::fs::create_dir(&pidfile).expect("directory-shaped pidfile should be created");
    assert!(read_regular_pidfile(&pidfile).is_err());

    let oversized_pidfile = root.path().join("oversized.pid");
    std::fs::write(&oversized_pidfile, "1".repeat(33))
        .expect("oversized pidfile should be written");
    assert!(read_regular_pidfile(&oversized_pidfile).is_err());

    #[cfg(unix)]
    {
        let target = root.path().join("pid-target");
        let link = root.path().join("linked.pid");
        std::fs::write(&target, "123").expect("pidfile target should be written");
        std::os::unix::fs::symlink(&target, &link).expect("pidfile symlink should be created");
        assert!(read_regular_pidfile(&link).is_err());
    }
}

#[test]
fn configured_runtime_signal_accepts_only_named_signal_numbers() {
    assert_eq!(
        RuntimeProcessSignal::parse(" TERM ").unwrap().number(),
        libc::SIGTERM
    );
    assert_eq!(
        RuntimeProcessSignal::parse("SIGTERM").unwrap().number(),
        libc::SIGTERM
    );
    assert_eq!(
        RuntimeProcessSignal::parse(&libc::SIGTERM.to_string())
            .unwrap()
            .number(),
        libc::SIGTERM
    );
    assert_eq!(RuntimeProcessSignal::kill().number(), libc::SIGKILL);

    for rejected in ["", "0", "-1", "999", "RTMIN", "SIGRTMIN"] {
        assert!(
            RuntimeProcessSignal::parse(rejected).is_err(),
            "signal {rejected:?} must fail closed"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn pidfd_signal_targets_only_the_exact_process_incarnation() {
    let (root, mut sentinel, state) = fixture();
    let identity = capture_fixture_identity(&root, &sentinel, &state);

    assert_eq!(
        signal_authenticated_runtime_process(
            &identity,
            &state,
            &root.path().join("runtime.pid"),
            RuntimeProcessSignal::kill(),
        )
        .expect("exact pidfd signal should complete"),
        RuntimeProcessSignalOutcome::Delivered
    );
    sentinel.wait_for_exit();
}

#[cfg(target_os = "linux")]
#[test]
fn substituted_birth_never_authorizes_a_pidfd_signal() {
    let (root, mut sentinel, state) = fixture();
    let identity =
        capture_fixture_identity(&root, &sentinel, &state).with_substituted_birth_for_test();

    assert!(
        signal_authenticated_runtime_process(
            &identity,
            &state,
            &root.path().join("runtime.pid"),
            RuntimeProcessSignal::kill(),
        )
        .is_err()
    );
    sentinel.assert_alive();
}

#[cfg(not(target_os = "linux"))]
#[test]
fn unsupported_platform_signal_capability_fails_closed() {
    let (root, mut sentinel, state) = fixture();
    let identity = capture_fixture_identity(&root, &sentinel, &state);

    assert!(matches!(
        signal_authenticated_runtime_process(
            &identity,
            &state,
            &root.path().join("runtime.pid"),
            RuntimeProcessSignal::kill(),
        ),
        Err(SandboxError::BackendUnavailable { .. })
    ));
    sentinel.assert_alive();
}

#[test]
fn linux_proc_stat_parser_uses_birth_field_after_parenthesized_command() {
    let mut fields = vec!["S"; 20];
    fields[19] = "987654";
    let stat = format!("123 (worker ) command) {}", fields.join(" "));

    assert_eq!(
        parse_linux_process_birth(123, &stat).unwrap(),
        RuntimeProcessBirth::LinuxProcStartTicks { ticks: 987654 }
    );
}
