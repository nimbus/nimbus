//! Fresh-process proof for krun attachment-adoption recovery.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use nimbus_network::NetworkAttachmentReservationState;

use super::support::*;
use crate::backends::oci::network::default_network_attachment_id;
use crate::backends::poll::poll_until_deadline;

const CRASH_CHILD_TEST: &str =
    "backends::krun::vm::tests::attachment_recovery::krun_attachment_crash_child";
const RECOVERY_CHILD_TEST: &str =
    "backends::krun::vm::tests::attachment_recovery::krun_attachment_recovery_child";
const REPLAY_CHILD_TEST: &str =
    "backends::krun::vm::tests::attachment_recovery::krun_attachment_replay_child";
const ROOT_ENV: &str = "NIMBUS_NNC38_KRUN_ATTACHMENT_ROOT";
const CRASH_MARKER: &str = "krun-attachment-adopting.durable";
const RECOVERY_OBSERVATION: &str =
    "network.krun-attachment.fresh:reserved=released:adopted=promoted-then-released";
const REPLAY_OBSERVATION: &str = "network.krun-attachment.replay:reserved=stable:adopted=stable";
const RESERVED_CASE: &str = "reserved";
const ADOPTED_CASE: &str = "adopted";
const RESERVED_ID: &str = "fresh-krun-reserved";
const ADOPTED_ID: &str = "fresh-krun-adopted";
const CHILD_TIMEOUT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[test]
fn fresh_process_converges_exact_krun_adoption_matrix() {
    let root = TempDir::new().expect("shared krun crash root should exist");
    let mut crash = spawn_child(CRASH_CHILD_TEST, root.path());
    let marker = root.path().join(CRASH_MARKER);
    let reached = poll_until_deadline(Some(Instant::now() + CHILD_TIMEOUT), POLL_INTERVAL, || {
        Ok(marker.is_file().then_some(()))
    })
    .expect("krun crash-boundary polling should not fail");
    if reached.is_none() {
        terminate_child(&mut crash);
        let output = collect_child(crash);
        panic!(
            "crash child did not reach both durable Adopting boundaries within \
             {CHILD_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        );
    }
    terminate_child(&mut crash);
    let crash = collect_child(crash);
    assert!(
        !crash.status.success(),
        "crash child must be killed while both lifecycle locks are held\nstdout:\n{}\nstderr:\n{}",
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
#[ignore = "spawned only by the NNC3.8 krun attachment crash-matrix parent"]
fn krun_attachment_crash_child() {
    let root = child_root();

    let (reserved_backend, reserved) = prepare_adopting_case(&root, RESERVED_CASE, RESERVED_ID);
    let reserved_lock = reserved_backend
        .lock_launch_lifecycle(&reserved)
        .expect("reserved crash child should hold exact lifecycle authority");
    reserved_backend
        .write_manifest(&reserved)
        .expect("reserved Adopting state should be durable");

    let (adopted_backend, adopted) = prepare_adopting_case(&root, ADOPTED_CASE, ADOPTED_ID);
    let adopted_lock = adopted_backend
        .lock_launch_lifecycle(&adopted)
        .expect("adopted crash child should hold exact lifecycle authority");
    adopted_backend
        .write_manifest(&adopted)
        .expect("adopted Adopting state should be durable before allocator commit");
    let adopted_claim = adopted
        .require_reserved_claim()
        .expect("adopted fixture should retain exact claim")
        .clone();
    adopted_backend
        .segment_allocator
        .adopt_reserved_attachment(
            &adopted.spec.tenant_id,
            &default_network_attachment_id(&adopted.handle.id),
            &adopted_claim,
        )
        .expect("allocator adoption should commit before owner death");

    let _retained = (
        reserved_lock,
        adopted_lock,
        reserved_backend,
        adopted_backend,
    );
    std::fs::write(root.join(CRASH_MARKER), b"durable\n")
        .expect("semantic crash marker should persist");
    loop {
        std::thread::park();
    }
}

#[test]
#[ignore = "spawned only by the NNC3.8 krun attachment crash-matrix parent"]
fn krun_attachment_recovery_child() {
    let root = child_root();

    let (reserved_backend, reserved) = load_case(&root, RESERVED_CASE, RESERVED_ID);
    let reserved_claim = reserved
        .require_reserved_claim()
        .expect("reserved recovery should retain exact claim")
        .clone();
    assert_eq!(
        reserved_backend
            .segment_allocator
            .inspect_attachment_reservation(
                &reserved.spec.tenant_id,
                &default_network_attachment_id(&reserved.handle.id),
                &reserved_claim,
            )
            .expect("reserved allocator outcome should inspect"),
        NetworkAttachmentReservationState::Reserved
    );
    reserved_backend
        .stop_sync(&reserved.handle.id)
        .expect("fresh reserved recovery should compensate exactly");
    let reserved = load_case(&root, RESERVED_CASE, RESERVED_ID).1;
    assert_eq!(reserved.launch_authority, KrunLaunchAuthority::Released);
    assert_eq!(reserved.status, SandboxStatus::Stopped);

    let (adopted_backend, adopted) = load_case(&root, ADOPTED_CASE, ADOPTED_ID);
    let adopted_claim = adopted
        .require_reserved_claim()
        .expect("adopted recovery should retain exact claim")
        .clone();
    assert_eq!(
        adopted_backend
            .segment_allocator
            .inspect_attachment_reservation(
                &adopted.spec.tenant_id,
                &default_network_attachment_id(&adopted.handle.id),
                &adopted_claim,
            )
            .expect("adopted allocator outcome should inspect"),
        NetworkAttachmentReservationState::ProviderCleanupPending,
        "fresh backend startup may quarantine the orphaned adopted hold, but the exact claim must \
         continue to prove the adopted side of the crash cut"
    );
    adopted_backend
        .stop_sync(&adopted.handle.id)
        .expect("fresh adopted recovery should promote before cleanup");
    let adopted = load_case(&root, ADOPTED_CASE, ADOPTED_ID).1;
    assert_eq!(adopted.launch_authority, KrunLaunchAuthority::Released);
    assert_eq!(adopted.status, SandboxStatus::Failed);

    println!("{RECOVERY_OBSERVATION}");
}

#[test]
#[ignore = "spawned only by the NNC3.8 krun attachment crash-matrix parent"]
fn krun_attachment_replay_child() {
    let root = child_root();
    for (case, id) in [(RESERVED_CASE, RESERVED_ID), (ADOPTED_CASE, ADOPTED_ID)] {
        let (backend, manifest) = load_case(&root, case, id);
        let before = std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("terminal manifest should read");
        backend
            .stop_sync(&manifest.handle.id)
            .expect("terminal attachment recovery should replay");
        assert_eq!(
            std::fs::read(&manifest.conmon_layout.manifest_path)
                .expect("replayed terminal manifest should read"),
            before,
            "{case} terminal replay must be byte-stable"
        );
    }
    println!("{REPLAY_OBSERVATION}");
}

fn prepare_adopting_case(
    root: &Path,
    case: &str,
    id: &str,
) -> (KrunSandboxBackend, KrunSandboxManifest) {
    let backend = KrunSandboxBackend::new(case_config(&root.join(case)));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec_for_tenant(&format!("krun-{case}-recovery"), "api"),
            &SandboxId::new(id),
            None,
            None,
        )
        .expect("crash fixture should reserve complete launch authority")
        .manifest;
    let claim = manifest
        .require_reserved_claim()
        .expect("crash fixture should retain exact claim")
        .clone();
    manifest.launch_authority = KrunLaunchAuthority::Adopting {
        reservation_claim: claim,
    };
    (backend, manifest)
}

fn load_case(root: &Path, case: &str, id: &str) -> (KrunSandboxBackend, KrunSandboxManifest) {
    let backend = KrunSandboxBackend::new(case_config(&root.join(case)));
    let manifest = backend
        .read_manifest(&SandboxId::new(id))
        .expect("fresh process should read krun manifest")
        .unwrap_or_else(|| panic!("krun manifest {id} should remain durable"));
    (backend, manifest)
}

fn case_config(root: &Path) -> KrunSandboxBackendConfig {
    let mut config = KrunSandboxBackendConfig::under_root(root.to_path_buf());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    config
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
        "fresh krun child failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        output.stdout,
        output.stderr
    );
    assert!(
        output.stdout.contains(expected),
        "fresh krun child omitted {expected:?}\nstdout:\n{}\nstderr:\n{}",
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
    PathBuf::from(std::env::var(ROOT_ENV).expect("shared krun crash root should be set"))
}
