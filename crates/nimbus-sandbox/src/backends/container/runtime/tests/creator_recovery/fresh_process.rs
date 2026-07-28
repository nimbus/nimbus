//! Fresh-process crash matrix for durable creator birth and containment.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::*;
use crate::backends::conmon::creator::CreatorAttemptReceipt;
use crate::backends::poll::poll_until_deadline;

const CRASH_CHILD_TEST: &str = "backends::container::runtime::tests::creator_recovery::\
fresh_process::creator_birth_crash_child";
const RECOVERY_CHILD_TEST: &str = "backends::container::runtime::tests::creator_recovery::\
fresh_process::creator_birth_recovery_child";
const DRAIN_RECOVERY_CHILD_TEST: &str = "backends::container::runtime::tests::creator_recovery::\
fresh_process::creator_birth_drain_recovery_child";
const ROOT_ENV: &str = "NIMBUS_NNC38_CREATOR_RECOVERY_ROOT";
const CRASH_BOUNDARY: &str = "network.creator-recovery.pending-receipts-durable";
const RECOVERY_OBSERVATION: &str = "network.creator-recovery.fresh:live=fenced:runtime=observed:escaped=fenced:\
unknown-birth=fenced:intent=quiesced";
const DRAIN_OBSERVATION: &str = "network.creator-recovery.drained:live=quiesced:escaped=quiesced:\
unknown-birth=dead-contained:intent=quiesced:runtime=observed";
const CHILD_TIMEOUT: Duration = Duration::from_secs(12);
const CONTAINMENT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

const LIVE_ID: &str = "fresh-creator-live";
const RUNTIME_OBSERVED_ID: &str = "fresh-creator-runtime-observed";
const ESCAPED_ID: &str = "fresh-creator-escaped";
const UNKNOWN_BIRTH_ID: &str = "fresh-creator-unknown-birth";
const INTENT_ONLY_ID: &str = "fresh-creator-intent-only";

const LIVE_RELEASE: &str = "live.release";
const RUNTIME_RELEASE: &str = "runtime.release";
const ESCAPED_START: &str = "escaped.start";
const ESCAPED_RELEASE: &str = "escaped.release";
const ESCAPED_DESCENDANT: &str = "escaped.descendant";
const UNKNOWN_BIRTH_RELEASE: &str = "unknown-birth.release";

#[test]
fn fresh_process_authenticates_creator_birth_and_containment_matrix() {
    let root = TempDir::new().expect("shared creator crash root should exist");
    let releases = ReleaseMarkers::new(root.path());
    let mut crash = spawn_child(CRASH_CHILD_TEST, root.path());
    let stdout = crash
        .stdout
        .take()
        .expect("crash child stdout should be piped");
    let stderr = crash
        .stderr
        .take()
        .expect("crash child stderr should be piped");
    let (line_sender, line_receiver) = mpsc::channel();
    let stdout_reader = std::thread::spawn(move || {
        let mut transcript = String::new();
        for line in BufReader::new(stdout).lines() {
            let line = line.expect("crash child stdout should remain readable");
            transcript.push_str(&line);
            transcript.push('\n');
            if line_sender.send(line).is_err() {
                break;
            }
        }
        transcript
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut transcript = String::new();
        BufReader::new(stderr)
            .read_to_string(&mut transcript)
            .expect("crash child stderr should remain readable");
        transcript
    });

    let deadline = Instant::now() + CHILD_TIMEOUT;
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            terminate_child(&mut crash);
            releases.release_all();
            let _ = crash.wait();
            let stdout = stdout_reader
                .join()
                .expect("crash stdout reader should join");
            let stderr = stderr_reader
                .join()
                .expect("crash stderr reader should join");
            panic!(
                "crash child exceeded {CHILD_TIMEOUT:?} before creator receipts became durable\n\
                 stdout:\n{stdout}\nstderr:\n{stderr}"
            );
        };
        match line_receiver.recv_timeout(remaining) {
            Ok(line) if line.contains(CRASH_BOUNDARY) => break,
            Ok(_) => {}
            Err(error) => {
                terminate_child(&mut crash);
                releases.release_all();
                let _ = crash.wait();
                let stdout = stdout_reader
                    .join()
                    .expect("crash stdout reader should join");
                let stderr = stderr_reader
                    .join()
                    .expect("crash stderr reader should join");
                panic!(
                    "crash child did not reach the durable creator-receipt boundary \
                     ({error:?})\nstdout:\n{stdout}\nstderr:\n{stderr}"
                );
            }
        }
    }

    terminate_child(&mut crash);
    let crash_status = crash.wait().expect("killed crash child should reap");
    let crash_stdout = stdout_reader
        .join()
        .expect("crash stdout reader should join");
    let crash_stderr = stderr_reader
        .join()
        .expect("crash stderr reader should join");
    assert!(
        !crash_status.success(),
        "crash child must be killed while creator authority remains pending\n\
         stdout:\n{crash_stdout}\nstderr:\n{crash_stderr}"
    );

    let recovery = run_child_to_completion(RECOVERY_CHILD_TEST, root.path());
    assert_child_observation(&recovery, RECOVERY_OBSERVATION);

    releases.release_all();
    let drain = run_child_to_completion(DRAIN_RECOVERY_CHILD_TEST, root.path());
    assert_child_observation(&drain, DRAIN_OBSERVATION);
}

#[test]
#[ignore = "spawned only by the NNC3.8 creator crash-matrix parent"]
fn creator_birth_crash_child() {
    let root = child_root();

    let live_release = root.join(LIVE_RELEASE);
    let (live_backend, mut live_manifest) = creator_recovery_fixture(&root, LIVE_ID);
    live_manifest.conmon_launch.state_command = explicit_absence_command(&live_manifest.handle.id);
    let live_creator = spawn_pending_creator(
        &live_backend,
        &mut live_manifest,
        "fresh-live-attempt",
        wait_for_marker_command(&live_release),
    );
    persist_dead_conmon_receipt(&live_manifest);

    let runtime_release = root.join(RUNTIME_RELEASE);
    let (runtime_backend, mut runtime_manifest) =
        creator_recovery_fixture(&root, RUNTIME_OBSERVED_ID);
    runtime_manifest.conmon_launch.state_command = exact_present_creator_command(
        &runtime_manifest.handle.id,
        "fresh-runtime-observed-attempt",
    );
    let mut runtime_creator = spawn_pending_creator(
        &runtime_backend,
        &mut runtime_manifest,
        "fresh-runtime-observed-attempt",
        wait_for_marker_command(&runtime_release),
    );
    write_marker(&runtime_release);
    runtime_creator
        .reap_after_runtime_observed(CONTAINMENT_TIMEOUT)
        .expect("runtime-observed creator should be reaped before the crash boundary");

    let escaped_start = root.join(ESCAPED_START);
    let escaped_release = root.join(ESCAPED_RELEASE);
    let escaped_descendant = root.join(ESCAPED_DESCENDANT);
    let (escaped_backend, mut escaped_manifest) = creator_recovery_fixture(&root, ESCAPED_ID);
    escaped_manifest.conmon_launch.state_command =
        explicit_absence_command(&escaped_manifest.handle.id);
    let mut escaped_creator = spawn_pending_creator(
        &escaped_backend,
        &mut escaped_manifest,
        "fresh-escaped-attempt",
        escaped_creator_command(&escaped_start, &escaped_release, &escaped_descendant),
    );
    persist_dead_conmon_receipt(&escaped_manifest);
    write_marker(&escaped_start);
    assert!(
        wait_for_path(&escaped_descendant, Duration::from_secs(2)),
        "escaped creator descendant receipt should become durable"
    );
    let escaped = escaped_creator
        .reap_after_runtime_observed(Duration::from_millis(100))
        .expect_err("live descendant must retain escaped containment");
    assert!(
        escaped.to_string().contains("process group")
            && escaped.to_string().contains("handoff remains pending"),
        "escaped crash fixture must reach the intended boundary: {escaped}"
    );

    let unknown_release = root.join(UNKNOWN_BIRTH_RELEASE);
    let (unknown_backend, mut unknown_manifest) = creator_recovery_fixture(&root, UNKNOWN_BIRTH_ID);
    unknown_manifest.conmon_launch.state_command =
        explicit_absence_command(&unknown_manifest.handle.id);
    let unknown_creator = spawn_pending_creator_with_receipt(
        &unknown_backend,
        &mut unknown_manifest,
        "fresh-unknown-birth-attempt",
        wait_for_marker_command(&unknown_release),
        CreatorAttemptReceipt::with_substituted_birth_for_test,
    );
    persist_dead_conmon_receipt(&unknown_manifest);

    let (intent_backend, mut intent_manifest) = creator_recovery_fixture(&root, INTENT_ONLY_ID);
    intent_manifest.creator_handoff = ContainerCreatorHandoffState::SpawnIntent {
        attempt_id: "fresh-intent-only-attempt".to_owned(),
    };
    intent_backend
        .write_manifest(&intent_manifest)
        .expect("intent-only creator manifest should become durable");

    let _retained_creator_authority = (
        live_creator,
        runtime_creator,
        escaped_creator,
        unknown_creator,
    );
    println!("{CRASH_BOUNDARY}");
    std::io::stdout()
        .flush()
        .expect("creator crash boundary should flush");
    loop {
        std::thread::park();
    }
}

#[test]
#[ignore = "spawned only by the NNC3.8 creator crash-matrix parent"]
fn creator_birth_recovery_child() {
    let root = child_root();

    assert_fenced(&root, LIVE_ID, &["remains live", "cleanup remains fenced"]);
    assert_fenced(&root, ESCAPED_ID, &["escaped", "cleanup remains fenced"]);
    assert_fenced(
        &root,
        UNKNOWN_BIRTH_ID,
        &["different process birth", "cleanup remains fenced"],
    );
    assert_intent_quiesces_once(&root, INTENT_ONLY_ID);

    let (backend, mut manifest) = load_manifest(&root, RUNTIME_OBSERVED_ID);
    let receipt = pending_receipt(&manifest);
    backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect("dead-contained creator plus exact runtime identity should promote");
    assert_eq!(
        manifest.creator_handoff,
        ContainerCreatorHandoffState::RuntimeObserved { receipt }
    );
    let first_bytes = read_manifest_bytes(&manifest);
    backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect("runtime-observed replay should be idempotent");
    assert_eq!(
        read_manifest_bytes(&manifest),
        first_bytes,
        "runtime-observed replay must keep canonical bytes stable"
    );

    println!("{RECOVERY_OBSERVATION}");
}

#[test]
#[ignore = "spawned only by the NNC3.8 creator crash-matrix parent"]
fn creator_birth_drain_recovery_child() {
    let root = child_root();

    wait_for_dead_containment(&root, LIVE_ID);
    assert_quiesces_once(&root, LIVE_ID);

    wait_for_dead_containment(&root, ESCAPED_ID);
    assert_quiesces_once(&root, ESCAPED_ID);

    wait_for_dead_containment(&root, UNKNOWN_BIRTH_ID);
    let (_, intent_manifest) = load_manifest(&root, INTENT_ONLY_ID);
    assert!(matches!(
        intent_manifest.creator_handoff,
        ContainerCreatorHandoffState::Quiesced {
            proof: CreatorQuiescenceProof::LaunchGateNeverReleased { .. }
        }
    ));

    let (_, runtime_manifest) = load_manifest(&root, RUNTIME_OBSERVED_ID);
    assert!(matches!(
        runtime_manifest.creator_handoff,
        ContainerCreatorHandoffState::RuntimeObserved { .. }
    ));

    println!("{DRAIN_OBSERVATION}");
}

fn spawn_pending_creator(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    attempt_id: &str,
    command: CommandSpec,
) -> OwnedConmonCreator {
    spawn_pending_creator_with_receipt(backend, manifest, attempt_id, command, |receipt| receipt)
}

fn spawn_pending_creator_with_receipt(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    attempt_id: &str,
    command: CommandSpec,
    transform: impl FnOnce(CreatorAttemptReceipt) -> CreatorAttemptReceipt,
) -> OwnedConmonCreator {
    let creator = OwnedConmonCreator::spawn_with_pid_receipt(
        &command,
        &manifest.conmon_layout.conmon_pidfile,
    )
    .expect("creator crash fixture should spawn");
    let receipt = transform(
        creator
            .attempt_receipt(attempt_id)
            .expect("creator birth/containment receipt should capture"),
    );
    manifest.creator_handoff = ContainerCreatorHandoffState::Pending { receipt };
    backend
        .write_manifest(manifest)
        .expect("pending creator receipt should become durable");
    creator
}

fn persist_dead_conmon_receipt(manifest: &ContainerSandboxManifest) {
    std::fs::write(
        &manifest.conmon_layout.conmon_pidfile,
        format!("{}\n", i32::MAX),
    )
    .expect("attempt-scoped dead conmon receipt should persist");
}

fn wait_for_marker_command(marker: &Path) -> CommandSpec {
    CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "while [ ! -f {} ]; do sleep 0.01; done",
            shell_words::quote(&marker.to_string_lossy())
        ),
    ])
}

fn escaped_creator_command(start: &Path, release: &Path, descendant_receipt: &Path) -> CommandSpec {
    CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "while [ ! -f {start} ]; do sleep 0.01; done; \
             (while [ ! -f {release} ]; do sleep 0.01; done) & descendant=$!; \
             printf '%s' \"$descendant\" > {receipt}; exit 0",
            start = shell_words::quote(&start.to_string_lossy()),
            release = shell_words::quote(&release.to_string_lossy()),
            receipt = shell_words::quote(&descendant_receipt.to_string_lossy()),
        ),
    ])
}

fn assert_fenced(root: &Path, id: &str, expected_fragments: &[&str]) {
    let (backend, mut manifest) = load_manifest(root, id);
    let before = manifest.creator_handoff.clone();
    let before_bytes = read_manifest_bytes(&manifest);
    let error = backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect_err("creator outcome should remain fenced");
    for fragment in expected_fragments {
        assert!(
            error.to_string().contains(fragment),
            "creator outcome for {id} omitted {fragment:?}: {error}"
        );
    }
    assert_eq!(
        manifest.creator_handoff, before,
        "fenced creator outcome for {id} must not mutate in-memory authority"
    );
    assert_eq!(
        read_manifest_bytes(&manifest),
        before_bytes,
        "fenced creator outcome for {id} must not mutate durable authority"
    );
}

fn assert_quiesces_once(root: &Path, id: &str) {
    let (backend, mut manifest) = load_manifest(root, id);
    let receipt = pending_receipt(&manifest);
    backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .unwrap_or_else(|error| panic!("dead-contained creator {id} should quiesce: {error}"));
    assert_eq!(
        manifest.creator_handoff,
        ContainerCreatorHandoffState::Quiesced {
            proof: CreatorQuiescenceProof::dead_contained(receipt),
        }
    );
    let first_bytes = read_manifest_bytes(&manifest);
    backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect("quiesced creator replay should be idempotent");
    assert_eq!(
        read_manifest_bytes(&manifest),
        first_bytes,
        "quiesced creator replay for {id} must keep canonical bytes stable"
    );
}

fn assert_intent_quiesces_once(root: &Path, id: &str) {
    let (backend, mut manifest) = load_manifest(root, id);
    let attempt_id = match &manifest.creator_handoff {
        ContainerCreatorHandoffState::SpawnIntent { attempt_id } => attempt_id.clone(),
        state => panic!("expected durable SpawnIntent creator handoff, found {state:?}"),
    };
    backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect("unreleased launch-gate intent should quiesce");
    assert_eq!(
        manifest.creator_handoff,
        ContainerCreatorHandoffState::Quiesced {
            proof: CreatorQuiescenceProof::launch_gate_never_released(attempt_id),
        }
    );
    let first_bytes = read_manifest_bytes(&manifest);
    backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect("launch-gate quiescence replay should be idempotent");
    assert_eq!(read_manifest_bytes(&manifest), first_bytes);
}

fn wait_for_dead_containment(root: &Path, id: &str) {
    let (_, manifest) = load_manifest(root, id);
    let receipt = pending_receipt(&manifest);
    let observed = poll_until_deadline(
        Some(Instant::now() + CONTAINMENT_TIMEOUT),
        POLL_INTERVAL,
        || {
            Ok(matches!(
                observe_creator_containment(&receipt),
                CreatorContainmentObservation::DeadContained
            )
            .then_some(()))
        },
    )
    .expect("creator containment polling should not fail");
    assert!(
        observed.is_some(),
        "creator {id} did not reach dead-contained within {CONTAINMENT_TIMEOUT:?}; last \
         observation: {:?}",
        observe_creator_containment(&receipt)
    );
}

fn pending_receipt(manifest: &ContainerSandboxManifest) -> CreatorAttemptReceipt {
    match &manifest.creator_handoff {
        ContainerCreatorHandoffState::Pending { receipt } => receipt.clone(),
        state => panic!("expected durable Pending creator handoff, found {state:?}"),
    }
}

fn load_manifest(root: &Path, id: &str) -> (ContainerSandboxBackend, ContainerSandboxManifest) {
    let backend = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(root));
    let manifest = backend
        .read_manifest(&SandboxId::new(id))
        .expect("fresh process should read the creator manifest")
        .unwrap_or_else(|| panic!("creator manifest {id} should remain durable"));
    (backend, manifest)
}

fn read_manifest_bytes(manifest: &ContainerSandboxManifest) -> Vec<u8> {
    std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("creator manifest bytes should remain readable")
}

struct ReleaseMarkers {
    paths: Vec<PathBuf>,
}

impl ReleaseMarkers {
    fn new(root: &Path) -> Self {
        Self {
            paths: [
                LIVE_RELEASE,
                RUNTIME_RELEASE,
                ESCAPED_START,
                ESCAPED_RELEASE,
                UNKNOWN_BIRTH_RELEASE,
            ]
            .into_iter()
            .map(|name| root.join(name))
            .collect(),
        }
    }

    fn release_all(&self) {
        for path in &self.paths {
            let _ = std::fs::write(path, b"release\n");
        }
    }
}

impl Drop for ReleaseMarkers {
    fn drop(&mut self) {
        self.release_all();
    }
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
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(POLL_INTERVAL);
            }
            Ok(None) => {
                terminate_child(&mut child);
                let _ = child.wait();
                panic!("child test {test_name} exceeded {CHILD_TIMEOUT:?}");
            }
            Err(error) => {
                terminate_child(&mut child);
                let _ = child.wait();
                panic!("failed to wait for child test {test_name}: {error}");
            }
        }
    };
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
        "fresh creator child failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        output.stdout,
        output.stderr
    );
    assert!(
        output.stdout.contains(expected),
        "fresh creator child omitted {expected:?}\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
}

fn terminate_child(child: &mut Child) {
    match child.kill() {
        Ok(()) => {}
        Err(error) if matches!(error.kind(), std::io::ErrorKind::InvalidInput) => {}
        Err(error) => panic!("failed to terminate child {}: {error}", child.id()),
    }
}

fn child_root() -> PathBuf {
    PathBuf::from(std::env::var(ROOT_ENV).expect("shared creator crash root should be set"))
}

fn write_marker(path: &Path) {
    std::fs::write(path, b"ready\n").unwrap_or_else(|error| {
        panic!(
            "failed to write semantic marker {}: {error}",
            path.display()
        )
    });
}
