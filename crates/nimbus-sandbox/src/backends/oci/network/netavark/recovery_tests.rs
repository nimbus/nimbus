//! Recovery proofs for pending Netavark provider operations.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use nimbus_core::TenantId;
use nimbus_network::LocalNetworkStateStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::tempdir;

use super::super::ipam::{
    allocate_container_ips, begin_netavark_setup, begin_netavark_teardown, complete_netavark_setup,
    complete_netavark_teardown, confirm_netavark_provider_detached,
    deallocate_container_ips_after_confirmed_detach, inspect_netavark_provider_operation,
};
use super::super::layout::{OciNetworkConfig, OciNetworkLayout};
use super::*;
use crate::backends::oci::port_lease::new_launch_reservation_claim;

const CRASH_CHILD_TEST: &str =
    "backends::oci::network::netavark::recovery_tests::netavark_response_loss_crash_child";
const RECOVERY_CHILD_TEST: &str =
    "backends::oci::network::netavark::recovery_tests::netavark_response_loss_recovery_child";
const REPLAY_CHILD_TEST: &str =
    "backends::oci::network::netavark::recovery_tests::netavark_response_loss_replay_child";
const ROOT_ENV: &str = "NIMBUS_NNC38_NETAVARK_RECOVERY_ROOT";
const CRASH_MARKER: &str = "netavark-response-loss.durable";
const RECOVERY_OBSERVATION: &str =
    "network.netavark.fresh:setup=compensated:delete=observed-absent:reuse=fenced";
const REPLAY_OBSERVATION: &str =
    "network.netavark.replay:setup=stable:delete=stable:replacement=admitted";
const SETUP_CASE: &str = "setup";
const DELETE_CASE: &str = "delete";
const SETUP_ID: &str = "netavark-setup-response-loss";
const DELETE_ID: &str = "netavark-delete-response-loss";
const CHILD_TIMEOUT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestProviderEvidence {
    operation: super::super::dto::NetavarkProviderOperation,
    effect: TestProviderEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TestProviderEffect {
    Present,
    Absent,
}

#[test]
fn stale_setup_claim_cannot_take_over_deleting_or_detached_projection_cleanup() {
    let root = tempdir().expect("network state root should create");
    let tenant =
        TenantId::new("tenant-netavark-stale-setup-claim").expect("tenant should validate");
    let sandbox = SandboxId::new("netavark-stale-setup-claim");
    let layout = OciNetworkLayout::new(root.path(), &tenant, &sandbox);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");

    let (_, stale_setup_claim) =
        begin_netavark_setup(&layout, &config, &sandbox).expect("first setup should begin");
    complete_netavark_setup(&layout, &stale_setup_claim).expect("first setup should publish Ready");
    let first_teardown = match begin_netavark_teardown(&layout, &config, &sandbox, None)
        .expect("first teardown should begin")
    {
        NetavarkTeardownPlan::Run { claim, .. } => claim,
        _ => panic!("Ready authority must begin provider teardown"),
    };
    confirm_netavark_provider_detached(&layout, &first_teardown)
        .expect("first provider absence should publish");
    complete_netavark_teardown(&layout, &first_teardown)
        .expect("first projection removal should complete");

    let (_, current_setup_claim) =
        begin_netavark_setup(&layout, &config, &sandbox).expect("second setup should begin");
    complete_netavark_setup(&layout, &current_setup_claim)
        .expect("second setup should publish Ready");
    let current_teardown = match begin_netavark_teardown(&layout, &config, &sandbox, None)
        .expect("current teardown should begin")
    {
        NetavarkTeardownPlan::Run { claim, .. } => claim,
        _ => panic!("current Ready authority must begin provider teardown"),
    };

    let deleting = inspect_netavark_provider_operation(&layout, &config, &sandbox)
        .expect("Deleting authority should inspect");
    let deleting_error =
        match begin_netavark_teardown(&layout, &config, &sandbox, Some(&stale_setup_claim)) {
            Ok(_) => panic!("stale setup capability must not take over Deleting"),
            Err(error) => error,
        };
    assert!(
        deleting_error.to_string().contains("does not own"),
        "rejection must identify the stale setup capability: {deleting_error}"
    );
    assert_eq!(
        inspect_netavark_provider_operation(&layout, &config, &sandbox)
            .expect("Deleting authority should remain inspectable"),
        deleting,
        "stale setup rejection must not mutate the current delete attempt"
    );

    confirm_netavark_provider_detached(&layout, &current_teardown)
        .expect("current provider absence should publish");
    let detached_projection = inspect_netavark_provider_operation(&layout, &config, &sandbox)
        .expect("DetachedProjectionPending authority should inspect");
    let projection_error =
        match begin_netavark_teardown(&layout, &config, &sandbox, Some(&stale_setup_claim)) {
            Ok(_) => panic!("stale setup capability must not remove a current projection"),
            Err(error) => error,
        };
    assert!(
        projection_error.to_string().contains("does not own"),
        "rejection must identify the stale setup capability: {projection_error}"
    );
    assert_eq!(
        inspect_netavark_provider_operation(&layout, &config, &sandbox)
            .expect("pending projection authority should remain inspectable"),
        detached_projection,
        "stale setup rejection must not mutate the current projection-removal attempt"
    );
}

#[test]
fn reopened_deleting_reuses_the_exact_attempt_instead_of_staying_pending() {
    let root = tempdir().expect("network state root should create");
    let tenant = TenantId::new("tenant-netavark-delete-recovery").expect("tenant should validate");
    let sandbox = SandboxId::new("netavark-delete-recovery");
    let layout = OciNetworkLayout::new(root.path(), &tenant, &sandbox);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    setup_container_network_with_runner(&layout, &config, &sandbox, |action, _assigned_ips| {
        assert_eq!(action, "setup");
        Ok(Value::Null)
    })
    .expect("fixture setup should publish Ready provider authority");
    std::fs::write(&layout.netns_path, b"current-netns")
        .expect("provider namespace marker should exist");

    let first_claim = match begin_netavark_teardown(&layout, &config, &sandbox, None)
        .expect("first owner should durably publish Deleting before its provider effect")
    {
        NetavarkTeardownPlan::Run { claim, .. } => claim,
        _ => panic!("Ready provider authority must begin one teardown attempt"),
    };

    let calls = AtomicUsize::new(0);
    let recovered = begin_netavark_teardown(&layout, &config, &sandbox, None)
        .expect("fresh owner should inspect the exact durable delete attempt");
    let recovered_claim = match &recovered {
        NetavarkTeardownPlan::Run { claim, .. } => claim,
        _ => panic!("Deleting authority must resume provider teardown"),
    };
    assert_eq!(
        recovered_claim, &first_claim,
        "fresh recovery must reuse the exact attempt instead of minting a replacement"
    );
    execute_teardown_plan(&layout, recovered, &mut |action, _assigned_ips| {
        assert_eq!(action, "teardown");
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(Value::Null)
    })
    .expect("fresh owner should resume the exact durable delete attempt");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "recovery must retry one exact teardown and never rerun setup"
    );
}

#[test]
fn reopened_provisioning_compensates_the_exact_attempt_without_duplicate_setup() {
    let root = tempdir().expect("network state root should create");
    let tenant = TenantId::new("tenant-netavark-setup-recovery").expect("tenant should validate");
    let sandbox = SandboxId::new("netavark-setup-recovery");
    let layout = OciNetworkLayout::new(root.path(), &tenant, &sandbox);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    let _lost_setup_claim = begin_netavark_setup(&layout, &config, &sandbox)
        .expect("first owner should durably publish Provisioning");
    std::fs::write(&layout.netns_path, b"provider-created-before-response-loss")
        .expect("provider effect marker should exist");

    let setup_calls = AtomicUsize::new(0);
    let teardown_calls = AtomicUsize::new(0);
    teardown_container_network_with_runner(&layout, &config, &sandbox, |action, _assigned_ips| {
        match action {
            "setup" => {
                setup_calls.fetch_add(1, Ordering::SeqCst);
            }
            "teardown" => {
                teardown_calls.fetch_add(1, Ordering::SeqCst);
            }
            other => panic!("unexpected Netavark action {other}"),
        }
        Ok(Value::Null)
    })
    .expect("fresh owner should compensate the exact pending setup generation");

    assert_eq!(
        setup_calls.load(Ordering::SeqCst),
        0,
        "response loss must never cause a duplicate setup effect"
    );
    assert_eq!(
        teardown_calls.load(Ordering::SeqCst),
        1,
        "the exact pending setup must converge through one teardown compensation"
    );
}

#[test]
fn fresh_process_converges_netavark_response_loss_matrix() {
    let root = tempdir().expect("shared Netavark crash root should create");
    let mut crash = spawn_child(CRASH_CHILD_TEST, root.path());
    let marker = root.path().join(CRASH_MARKER);
    let reached = poll_until(CHILD_TIMEOUT, || marker.is_file());
    if !reached {
        terminate_child(&mut crash);
        let output = collect_child(crash);
        panic!(
            "Netavark crash child did not reach both response-loss cuts\nstdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        );
    }
    terminate_child(&mut crash);
    let crash = collect_child(crash);
    assert!(
        !crash.status.success(),
        "crash child must be killed after both durable response-loss cuts"
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
#[ignore = "spawned only by the NNC3.8 Netavark crash-matrix parent"]
fn netavark_response_loss_crash_child() {
    let root = child_root();

    let (setup_layout, setup_config, setup_sandbox) = initialize_case(&root, SETUP_CASE, SETUP_ID);
    allocate_container_ips(&setup_layout, &setup_config, &setup_sandbox)
        .expect("setup case should reserve exact IPAM");
    begin_netavark_setup(&setup_layout, &setup_config, &setup_sandbox)
        .expect("setup attempt should become durable before provider effect");
    std::fs::write(&setup_layout.netns_path, b"setup-effect-created")
        .expect("setup provider namespace should exist");
    write_provider_evidence(
        &root,
        SETUP_CASE,
        &TestProviderEvidence {
            operation: inspect_netavark_provider_operation(
                &setup_layout,
                &setup_config,
                &setup_sandbox,
            )
            .expect("setup provider generation should inspect"),
            effect: TestProviderEffect::Present,
        },
    );

    let (delete_layout, delete_config, delete_sandbox) =
        initialize_case(&root, DELETE_CASE, DELETE_ID);
    allocate_container_ips(&delete_layout, &delete_config, &delete_sandbox)
        .expect("delete case should reserve exact IPAM");
    setup_container_network_with_runner(
        &delete_layout,
        &delete_config,
        &delete_sandbox,
        |action, _| {
            assert_eq!(action, "setup");
            Ok(Value::Null)
        },
    )
    .expect("delete case should publish Ready provider authority");
    std::fs::write(&delete_layout.netns_path, b"delete-effect-started")
        .expect("delete provider namespace should exist");
    begin_netavark_teardown(&delete_layout, &delete_config, &delete_sandbox, None)
        .expect("delete attempt should become durable before provider effect");
    remove_provider_marker(&delete_layout.netns_path);
    write_provider_evidence(
        &root,
        DELETE_CASE,
        &TestProviderEvidence {
            operation: inspect_netavark_provider_operation(
                &delete_layout,
                &delete_config,
                &delete_sandbox,
            )
            .expect("delete provider generation should inspect"),
            effect: TestProviderEffect::Absent,
        },
    );

    persist_bytes(&root.join(CRASH_MARKER), b"durable\n");
    loop {
        std::thread::park();
    }
}

#[test]
#[ignore = "spawned only by the NNC3.8 Netavark crash-matrix parent"]
fn netavark_response_loss_recovery_child() {
    let root = child_root();

    recover_case(&root, SETUP_CASE, SETUP_ID, TestProviderEffect::Present);
    recover_case(&root, DELETE_CASE, DELETE_ID, TestProviderEffect::Absent);

    println!("{RECOVERY_OBSERVATION}");
}

#[test]
#[ignore = "spawned only by the NNC3.8 Netavark crash-matrix parent"]
fn netavark_response_loss_replay_child() {
    let root = child_root();
    let authority_path = LocalNetworkStateStore::authority_path_for(&root);
    let before = std::fs::read(&authority_path).expect("terminal network authority should read");

    for (case, id) in [(SETUP_CASE, SETUP_ID), (DELETE_CASE, DELETE_ID)] {
        let (layout, config, sandbox) = load_case(&root, case, id);
        teardown_container_network_with_runner(&layout, &config, &sandbox, |_, _| {
            panic!("terminal replay must not invoke Netavark")
        })
        .expect("terminal response-loss replay should remain idempotent");
    }
    assert_eq!(
        std::fs::read(&authority_path).expect("replayed network authority should read"),
        before,
        "terminal replay must preserve exact authority bytes"
    );

    for (case, id) in [(SETUP_CASE, SETUP_ID), (DELETE_CASE, DELETE_ID)] {
        let (layout, mut replacement, sandbox) = load_case(&root, case, id);
        replacement.reservation_claim =
            new_launch_reservation_claim().expect("replacement claim should mint");
        allocate_container_ips(&layout, &replacement, &sandbox)
            .expect("replacement may reserve only after exact terminal detach and IPAM release");
    }

    println!("{REPLAY_OBSERVATION}");
}

fn recover_case(root: &Path, case: &str, id: &str, expected_effect: TestProviderEffect) {
    let (layout, config, sandbox) = load_case(root, case, id);
    let evidence = read_provider_evidence(root, case);
    assert_eq!(evidence.effect, expected_effect);
    assert_eq!(
        inspect_netavark_provider_operation(&layout, &config, &sandbox)
            .expect("fresh process should inspect exact provider generation"),
        evidence.operation,
        "provider evidence must authenticate the durable operation generation"
    );
    let provider_marker_present = std::fs::symlink_metadata(&layout.netns_path).is_ok();
    assert_eq!(
        provider_marker_present,
        expected_effect == TestProviderEffect::Present,
        "persisted provider evidence must agree with the exact namespace effect"
    );

    let plan = begin_netavark_teardown(&layout, &config, &sandbox, None)
        .expect("fresh process should derive exact teardown reconciliation");
    let claim = match &plan {
        NetavarkTeardownPlan::Run { claim, .. } => claim,
        _ => panic!("pending provider operation must reconcile through exact teardown"),
    };
    match &evidence.operation {
        super::super::dto::NetavarkProviderOperation::Provisioning { operation_attempt } => {
            assert_eq!(
                claim.setup_attempt(),
                operation_attempt,
                "setup compensation must retain the exact lost setup attempt"
            );
        }
        super::super::dto::NetavarkProviderOperation::Deleting {
            setup_attempt,
            operation_attempt,
        } => {
            assert_eq!(claim.setup_attempt(), setup_attempt);
            assert_eq!(
                claim.operation_attempt(),
                operation_attempt,
                "delete response loss must replay the same operation attempt"
            );
        }
        other => panic!("unexpected pending operation evidence {other:?}"),
    }
    let teardown_calls = AtomicUsize::new(0);
    execute_teardown_plan(&layout, plan, &mut |action, _| {
        assert_eq!(action, "teardown");
        assert_eq!(
            expected_effect,
            TestProviderEffect::Present,
            "an already-absent exact provider effect must not be deleted again"
        );
        teardown_calls.fetch_add(1, Ordering::SeqCst);
        std::fs::remove_file(&layout.netns_path)
            .expect("the stateful provider substitute should commit exact deletion");
        Ok(Value::Null)
    })
    .expect("exact response-loss reconciliation should converge");
    assert_eq!(
        teardown_calls.load(Ordering::SeqCst),
        usize::from(expected_effect == TestProviderEffect::Present),
        "present setup compensation runs once while committed delete response loss runs zero times"
    );
    assert!(
        std::fs::symlink_metadata(&layout.netns_path).is_err(),
        "provider absence must precede projection completion and IPAM release"
    );

    let mut replacement = config.clone();
    replacement.reservation_claim =
        new_launch_reservation_claim().expect("replacement claim should mint");
    allocate_container_ips(&layout, &replacement, &sandbox)
        .expect_err("replacement must remain fenced before exact IPAM release");
    deallocate_container_ips_after_confirmed_detach(&layout, &sandbox, &config.reservation_claim)
        .expect("terminal provider absence should release exact IPAM");
}

fn initialize_case(
    root: &Path,
    case: &str,
    id: &str,
) -> (OciNetworkLayout, OciNetworkConfig, SandboxId) {
    let tenant = TenantId::new(format!("tenant-netavark-{case}-response-loss"))
        .expect("tenant should validate");
    let sandbox = SandboxId::new(id);
    let layout = OciNetworkLayout::new(root, &tenant, &sandbox);
    layout
        .ensure_directories()
        .expect("case network layout should create");
    let config = OciNetworkConfig::default();
    let case_root = root.join(case);
    std::fs::create_dir_all(&case_root).expect("case metadata root should create");
    std::fs::write(
        case_root.join("config.json"),
        serde_json::to_vec_pretty(&config).expect("case config should serialize"),
    )
    .expect("case config should persist");
    (layout, config, sandbox)
}

fn load_case(root: &Path, case: &str, id: &str) -> (OciNetworkLayout, OciNetworkConfig, SandboxId) {
    let tenant = TenantId::new(format!("tenant-netavark-{case}-response-loss"))
        .expect("tenant should validate");
    let sandbox = SandboxId::new(id);
    let layout = OciNetworkLayout::new(root, &tenant, &sandbox);
    let config = serde_json::from_slice(
        &std::fs::read(root.join(case).join("config.json"))
            .expect("case config should remain durable"),
    )
    .expect("case config should deserialize");
    (layout, config, sandbox)
}

fn evidence_path(root: &Path, case: &str) -> PathBuf {
    root.join(case).join("provider-evidence.json")
}

fn write_provider_evidence(root: &Path, case: &str, evidence: &TestProviderEvidence) {
    persist_bytes(
        &evidence_path(root, case),
        serde_json::to_vec_pretty(evidence).expect("provider evidence should serialize"),
    );
}

fn read_provider_evidence(root: &Path, case: &str) -> TestProviderEvidence {
    serde_json::from_slice(
        &std::fs::read(evidence_path(root, case)).expect("provider evidence should remain durable"),
    )
    .expect("provider evidence should deserialize")
}

fn remove_provider_marker(path: &Path) {
    std::fs::remove_file(path).expect("stateful provider substitute should commit deletion");
    let parent = path
        .parent()
        .expect("provider marker should have a parent directory");
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .expect("provider deletion should sync its parent directory");
}

fn persist_bytes(path: &Path, bytes: impl AsRef<[u8]>) {
    let mut file = std::fs::File::create(path).expect("durable fixture file should create");
    file.write_all(bytes.as_ref())
        .expect("durable fixture bytes should write");
    file.sync_all().expect("durable fixture bytes should sync");
    let parent = path
        .parent()
        .expect("durable fixture file should have a parent directory");
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .expect("durable fixture directory should sync");
}

struct ChildOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn spawn_child(test_name: &str, root: &Path) -> Child {
    Command::new(std::env::current_exe().expect("test executable should resolve"))
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
        .expect("child stdout should exist")
        .read_to_string(&mut stdout)
        .expect("child stdout should read");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("child stderr should exist")
        .read_to_string(&mut stderr)
        .expect("child stderr should read");
    ChildOutput {
        status,
        stdout,
        stderr,
    }
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
}

fn assert_child_observation(output: &ChildOutput, expected: &str) {
    assert!(
        output.status.success() && output.stdout.contains(expected),
        "child did not report {expected:?}\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        output.stdout,
        output.stderr
    );
}

fn child_root() -> PathBuf {
    PathBuf::from(std::env::var_os(ROOT_ENV).expect("child root env should be set"))
}

fn poll_until(timeout: Duration, predicate: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    predicate()
}
