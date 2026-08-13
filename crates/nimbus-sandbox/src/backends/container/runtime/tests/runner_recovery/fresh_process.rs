//! Fresh-process crash proof for the exact runner EffectsStarted matrix.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use super::*;
use crate::backends::poll::poll_until_deadline;

const CRASH_CHILD_TEST: &str = "backends::container::runtime::tests::lifecycle::runner_recovery::\
fresh_process::runner_effects_crash_child";
const RECOVERY_CHILD_TEST: &str = "backends::container::runtime::tests::lifecycle::runner_recovery::\
fresh_process::runner_effects_recovery_child";
const REPLAY_CHILD_TEST: &str = "backends::container::runtime::tests::lifecycle::runner_recovery::\
fresh_process::runner_effects_replay_child";
const ROOT_ENV: &str = "NIMBUS_NNC38_RUNNER_RECOVERY_ROOT";
const CRASH_MARKER: &str = "runner-effects-started.durable";
const RECOVERY_OBSERVATION: &str =
    "network.runner-recovery.fresh:present=published:absent=terminal:ambiguous=fenced";
const REPLAY_OBSERVATION: &str =
    "network.runner-recovery.replay:present=stable:absent=stable:ambiguous=stable";
const CHILD_TIMEOUT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

const PRESENT_CASE: &str = "present";
const ABSENT_CASE: &str = "absent";
const AMBIGUOUS_CASE: &str = "ambiguous";
const PRESENT_ID: &str = "fresh-runner-present";
const ABSENT_ID: &str = "fresh-runner-absent";
const AMBIGUOUS_ID: &str = "fresh-runner-ambiguous";

#[test]
fn fresh_process_converges_exact_runner_effect_matrix() {
    let root = TempDir::new().expect("shared runner crash root should exist");
    let mut crash = spawn_child(CRASH_CHILD_TEST, root.path());
    let marker = root.path().join(CRASH_MARKER);
    let reached = poll_until_deadline(Some(Instant::now() + CHILD_TIMEOUT), POLL_INTERVAL, || {
        Ok(marker.is_file().then_some(()))
    })
    .expect("runner crash-boundary polling should not fail");
    if reached.is_none() {
        terminate_child(&mut crash);
        let output = collect_child(crash);
        panic!(
            "crash child did not reach the durable EffectsStarted boundary within \
             {CHILD_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        );
    }
    terminate_child(&mut crash);
    let crash = collect_child(crash);
    assert!(
        !crash.status.success(),
        "crash child must be killed while its lifecycle locks are held\nstdout:\n{}\nstderr:\n{}",
        crash.stdout,
        crash.stderr
    );

    assert_child_observation(
        &run_child_to_completion(RECOVERY_CHILD_TEST, root.path()),
        RECOVERY_OBSERVATION,
    );
    assert_child_observation(
        &run_child_to_completion(REPLAY_CHILD_TEST, root.path()),
        REPLAY_OBSERVATION,
    );
}

#[test]
#[ignore = "spawned only by the NNC3.8 runner crash-matrix parent"]
fn runner_effects_crash_child() {
    let root = child_root();

    let present_root = case_root(&root, PRESENT_CASE);
    let (present_backend, mut present) = prepared_runner_fixture(&present_root, PRESENT_ID);
    let present_creator_attempt = "fresh-runner-present-creator";
    present.conmon_launch.state_command =
        exact_present_command(&present.handle.id, Some(present_creator_attempt));
    present_backend
        .write_manifest(&present)
        .expect("present command should be durable before handoff");
    let present_handoff = super::super::super::runner::persist_runner_execution_ownership(
        &present_backend,
        &mut present,
    )
    .expect("present runner should claim execution");
    super::super::super::runner::mark_runner_effects_started(&present, &present_handoff)
        .expect("present EffectsStarted boundary should be durable");
    let claim = present
        .launch_reservation_claim
        .clone()
        .expect("present runner should retain exact launch authority");
    present_backend
        .segment_allocator
        .adopt_reserved_attachment(
            &present.spec.tenant_id,
            &default_network_attachment_id(&present.handle.id),
            &claim,
        )
        .expect("present attachment should adopt");
    std::fs::write(&present.network_layout.status_path, b"{}\n")
        .expect("present Netavark projection should persist");
    std::fs::write(&present.network_layout.netns_path, b"fixture-netns\n")
        .expect("present namespace projection should persist");
    present_backend
        .ensure_egress_proxy_running_with_release_authority(
            &present,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&claim),
        )
        .expect("present PEP effect should start");
    present.creator_handoff = ContainerCreatorHandoffState::RuntimeObserved {
        receipt: CreatorAttemptReceipt::for_test(present_creator_attempt),
    };
    present.launch_reservation_claim = None;
    present_backend
        .write_manifest(&present)
        .expect("complete present result should be durable");

    let absent_root = case_root(&root, ABSENT_CASE);
    let (absent_backend, mut absent) = prepared_runner_fixture(&absent_root, ABSENT_ID);
    mark_runtime_absent_for_cleanup(&mut absent);
    absent_backend
        .write_manifest(&absent)
        .expect("absent command should be durable before handoff");
    let absent_handoff = super::super::super::runner::persist_runner_execution_ownership(
        &absent_backend,
        &mut absent,
    )
    .expect("absent runner should claim execution");
    super::super::super::runner::mark_runner_effects_started(&absent, &absent_handoff)
        .expect("absent EffectsStarted boundary should be durable");

    let ambiguous_root = case_root(&root, AMBIGUOUS_CASE);
    let (ambiguous_backend, mut ambiguous) = prepared_runner_fixture(&ambiguous_root, AMBIGUOUS_ID);
    ambiguous.conmon_launch.state_command = ambiguous_runtime_command();
    ambiguous_backend
        .write_manifest(&ambiguous)
        .expect("ambiguous command should be durable before handoff");
    let ambiguous_handoff = super::super::super::runner::persist_runner_execution_ownership(
        &ambiguous_backend,
        &mut ambiguous,
    )
    .expect("ambiguous runner should claim execution");
    super::super::super::runner::mark_runner_effects_started(&ambiguous, &ambiguous_handoff)
        .expect("ambiguous EffectsStarted boundary should be durable");

    let _retained_lifecycle_locks = (
        present_handoff,
        absent_handoff,
        ambiguous_handoff,
        present_backend,
        absent_backend,
        ambiguous_backend,
    );
    std::fs::write(root.join(CRASH_MARKER), b"durable\n")
        .expect("semantic crash marker should persist");
    loop {
        std::thread::park();
    }
}

#[test]
#[ignore = "spawned only by the NNC3.8 runner crash-matrix parent"]
fn runner_effects_recovery_child() {
    let root = child_root();

    let (present_backend, mut present) = load_case(&root, PRESENT_CASE, PRESENT_ID);
    let present_handoff = super::super::super::runner::lock_execute_lifecycle(&present)
        .expect("fresh present recovery should acquire the dead owner's lock");
    assert_eq!(
        super::super::super::runner::reconcile_runner_effects_started(
            &present_backend,
            &mut present,
            &present_handoff,
        )
        .expect("fresh exact-present recovery should promote"),
        super::super::super::runner::RunnerEffectOutcome::Present
    );
    assert_eq!(
        super::super::super::runner::execute_handoff_phase(&present)
            .expect("present publication should authenticate"),
        None
    );

    let (_, absent_before) = load_case(&root, ABSENT_CASE, ABSENT_ID);
    let absent_error = super::super::super::run_prepared_container_service_workload(
        &absent_before.bundle_layout.bundle_dir,
    )
    .expect_err("fresh runner entrypoint should report the explicitly absent launch");
    assert!(
        absent_error.to_string().contains("explicitly absent")
            && absent_error
                .to_string()
                .contains("without replaying launch"),
        "production runner recovery must name the no-replay outcome: {absent_error}"
    );
    let (_, absent) = load_case(&root, ABSENT_CASE, ABSENT_ID);
    assert!(absent.has_terminal_network_finality());

    let (ambiguous_backend, mut ambiguous) = load_case(&root, AMBIGUOUS_CASE, AMBIGUOUS_ID);
    let ambiguous_handoff = super::super::super::runner::lock_execute_lifecycle(&ambiguous)
        .expect("fresh ambiguous recovery should acquire the dead owner's lock");
    let manifest_before = read_manifest_bytes(&ambiguous);
    let decision_before = read_decision_bytes(&ambiguous);
    let error = super::super::super::runner::reconcile_runner_effects_started(
        &ambiguous_backend,
        &mut ambiguous,
        &ambiguous_handoff,
    )
    .expect_err("generic fresh-process observation must remain ambiguous");
    assert!(
        error
            .to_string()
            .contains("without explicit absence evidence")
            && error.to_string().contains("remain fenced"),
        "ambiguous fresh-process diagnostic should retain exact authority: {error}"
    );
    assert_eq!(read_manifest_bytes(&ambiguous), manifest_before);
    assert_eq!(read_decision_bytes(&ambiguous), decision_before);

    println!("{RECOVERY_OBSERVATION}");
}

#[test]
#[ignore = "spawned only by the NNC3.8 runner crash-matrix parent"]
fn runner_effects_replay_child() {
    let root = child_root();

    let (_, present) = load_case(&root, PRESENT_CASE, PRESENT_ID);
    let present_manifest = read_manifest_bytes(&present);
    let present_decision = read_decision_bytes(&present);
    assert_eq!(
        super::super::super::runner::execute_handoff_phase(&present)
            .expect("published present handoff should authenticate"),
        None
    );
    assert_eq!(read_manifest_bytes(&present), present_manifest);
    assert_eq!(read_decision_bytes(&present), present_decision);

    let (absent_backend, absent) = load_case(&root, ABSENT_CASE, ABSENT_ID);
    let absent_manifest = read_manifest_bytes(&absent);
    let absent_decision = read_decision_bytes(&absent);
    absent_backend
        .inspect_sync(&absent.handle.id)
        .expect("terminal absent recovery should inspect")
        .expect("terminal absent recovery should remain durable");
    assert_eq!(read_manifest_bytes(&absent), absent_manifest);
    assert_eq!(read_decision_bytes(&absent), absent_decision);

    let (ambiguous_backend, mut ambiguous) = load_case(&root, AMBIGUOUS_CASE, AMBIGUOUS_ID);
    let ambiguous_handoff = super::super::super::runner::lock_execute_lifecycle(&ambiguous)
        .expect("ambiguous replay should reacquire lifecycle authority");
    let ambiguous_manifest = read_manifest_bytes(&ambiguous);
    let ambiguous_decision = read_decision_bytes(&ambiguous);
    super::super::super::runner::reconcile_runner_effects_started(
        &ambiguous_backend,
        &mut ambiguous,
        &ambiguous_handoff,
    )
    .expect_err("ambiguous replay must remain fenced");
    assert_eq!(read_manifest_bytes(&ambiguous), ambiguous_manifest);
    assert_eq!(read_decision_bytes(&ambiguous), ambiguous_decision);

    println!("{REPLAY_OBSERVATION}");
}

fn case_root(root: &Path, case: &str) -> PathBuf {
    root.join(case)
}

fn load_case(
    root: &Path,
    case: &str,
    id: &str,
) -> (ContainerSandboxBackend, ContainerSandboxManifest) {
    let case_root = case_root(root, case);
    let mut config = ContainerSandboxBackendConfig::plan_only(
        case_root.join("bundles"),
        case_root.join("state"),
    );
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .read_manifest(&SandboxId::new(id))
        .expect("fresh runner process should read the manifest")
        .unwrap_or_else(|| panic!("runner manifest {id} should remain durable"));
    (backend, manifest)
}

fn read_manifest_bytes(manifest: &ContainerSandboxManifest) -> Vec<u8> {
    std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("runner manifest bytes should remain readable")
}

fn read_decision_bytes(manifest: &ContainerSandboxManifest) -> Vec<u8> {
    std::fs::read(
        manifest
            .conmon_layout
            .container_state_dir
            .join(super::super::super::runner::RUNNER_HANDOFF_DECISION_FILE),
    )
    .expect("runner decision bytes should remain readable")
}

struct ChildOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn spawn_child(test_name: &str, root: &Path) -> Child {
    Command::new(std::env::current_exe().expect("current test executable should resolve"))
        .arg("--exact")
        .arg(test_name)
        .arg("--ignored")
        .arg("--nocapture")
        .env(ROOT_ENV, root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("failed to spawn child test {test_name}: {error}"))
}

fn run_child_to_completion(test_name: &str, root: &Path) -> ChildOutput {
    let mut child = spawn_child(test_name, root);
    let deadline = Instant::now() + CHILD_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return collect_child(child),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                terminate_child(&mut child);
                let output = collect_child(child);
                panic!(
                    "child test {test_name} exceeded {CHILD_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
                    output.stdout, output.stderr
                );
            }
            Err(error) => {
                terminate_child(&mut child);
                let output = collect_child(child);
                panic!(
                    "failed to wait for child test {test_name}: {error}\nstdout:\n{}\nstderr:\n{}",
                    output.stdout, output.stderr
                );
            }
        }
    }
}

fn collect_child(mut child: Child) -> ChildOutput {
    let status = child.wait().expect("child should reap");
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("child stdout should be piped")
        .read_to_string(&mut stdout)
        .expect("child stdout should remain readable");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("child stderr should be piped")
        .read_to_string(&mut stderr)
        .expect("child stderr should remain readable");
    ChildOutput {
        status,
        stdout,
        stderr,
    }
}

fn assert_child_observation(output: &ChildOutput, expected: &str) {
    assert!(
        output.status.success(),
        "fresh runner child failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        output.stdout,
        output.stderr
    );
    assert!(
        output.stdout.contains(expected),
        "fresh runner child omitted {expected:?}\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
}

fn terminate_child(child: &mut Child) {
    match child.kill() {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {}
        Err(error) => panic!("failed to terminate child {}: {error}", child.id()),
    }
}

fn child_root() -> PathBuf {
    PathBuf::from(std::env::var(ROOT_ENV).expect("shared runner crash root should be set"))
}
