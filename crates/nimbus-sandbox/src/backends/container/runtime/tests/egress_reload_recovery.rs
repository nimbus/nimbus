//! Fresh-process crash proof for acknowledged egress-policy reloads.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nimbus_egress::{EgressPolicy, EgressProtocol, EgressRule};
use nimbus_network::{LocalPortLeaseAuthority, PortLeaseEffectScope, PortLeasePhase};
use tempfile::TempDir;

use super::support::*;
use super::*;
use crate::backends::oci::egress::PepPreAdoptionReleaseAuthority;

const CRASH_CHILD_TEST: &str =
    "backends::container::runtime::tests::egress_reload_recovery::egress_reload_crash_child";
const RECOVERY_CHILD_TEST: &str =
    "backends::container::runtime::tests::egress_reload_recovery::egress_reload_recovery_child";
const ROOT_ENV: &str = "NIMBUS_NNC38_EGRESS_RELOAD_ROOT";
const PORT_ENV: &str = "NIMBUS_NNC38_EGRESS_RELOAD_PORT";
const SANDBOX_ID: &str = "fresh-process-egress-reload";
const ACKNOWLEDGED_BOUNDARY: &str = "network.egress-reload.provider-acknowledged";
const RECOVERED_OBSERVATION: &str =
    "network.egress-reload.recovered:desired=2:attempt=1:lifetime=2:stable";
const CHILD_TIMEOUT: Duration = Duration::from_secs(10);

#[test]
fn fresh_process_recovers_acknowledged_egress_reload_without_rollback_or_duplicate_attempt() {
    let root = TempDir::new().expect("shared crash state root should exist");
    let port = unused_loopback_port();
    let mut crash = spawn_child(CRASH_CHILD_TEST, root.path(), port);
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
            let _ = crash.wait();
            let stdout = stdout_reader
                .join()
                .expect("crash stdout reader should join");
            let stderr = stderr_reader
                .join()
                .expect("crash stderr reader should join");
            panic!(
                "crash child exceeded {CHILD_TIMEOUT:?} before the exact acknowledgement \
                 boundary\nstdout:\n{stdout}\nstderr:\n{stderr}"
            );
        };
        match line_receiver.recv_timeout(remaining) {
            Ok(line) if line.contains(ACKNOWLEDGED_BOUNDARY) => break,
            Ok(_) => {}
            Err(error) => {
                terminate_child(&mut crash);
                let _ = crash.wait();
                let stdout = stdout_reader
                    .join()
                    .expect("crash stdout reader should join");
                let stderr = stderr_reader
                    .join()
                    .expect("crash stderr reader should join");
                panic!(
                    "crash child did not reach exact acknowledgement boundary ({error:?})\n\
                     stdout:\n{stdout}\nstderr:\n{stderr}"
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
        "crash child must be killed at the provider-acknowledged boundary\n\
         stdout:\n{crash_stdout}\nstderr:\n{crash_stderr}"
    );

    let recovery = run_child_to_completion(RECOVERY_CHILD_TEST, root.path(), port);
    assert!(
        recovery.status.success(),
        "fresh recovery child failed with {}\nstdout:\n{}\nstderr:\n{}",
        recovery.status,
        recovery.stdout,
        recovery.stderr
    );
    assert!(
        recovery.stdout.contains(RECOVERED_OBSERVATION),
        "fresh recovery child did not report the exact converged state\nstdout:\n{}\nstderr:\n{}",
        recovery.stdout,
        recovery.stderr
    );
}

#[test]
#[ignore = "spawned only by the NNC3.8 fresh-process crash parent"]
fn egress_reload_crash_child() {
    let (root, port) = child_config();
    let config = backend_config(&root, port);
    let baseline = ContainerSandboxBackend::new(config);
    let mut manifest = baseline
        .plan_start_with_id(&sample_spec(), &SandboxId::new(SANDBOX_ID), None, None)
        .expect("crash child execute manifest should lower")
        .manifest;
    baseline
        .write_manifest(&manifest)
        .expect("crash child baseline manifest should publish");
    baseline
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("fresh execute plan should retain its reservation claim"),
            ),
        )
        .expect("crash child baseline PEP should start");
    manifest.launch_reservation_claim = None;
    baseline
        .write_manifest(&manifest)
        .expect("crash child post-launch manifest should publish");

    let backend = baseline.with_post_egress_reload_ack_observer(|| {
        println!("{ACKNOWLEDGED_BOUNDARY}");
        std::io::stdout()
            .flush()
            .expect("acknowledgement boundary should flush");
        loop {
            std::thread::park();
        }
    });
    backend
        .reload_egress_policy(&manifest.handle.id, desired_policy())
        .expect("parent must kill the child before acknowledged reload returns");
    panic!("crash child crossed the parent-owned kill boundary");
}

#[test]
#[ignore = "spawned only by the NNC3.8 fresh-process crash parent"]
fn egress_reload_recovery_child() {
    let (root, port) = child_config();
    let config = backend_config(&root, port);
    let backend = ContainerSandboxBackend::new(config.clone());
    let sandbox_id = SandboxId::new(SANDBOX_ID);
    let before = backend
        .read_manifest(&sandbox_id)
        .expect("fresh recovery should read the durable manifest")
        .expect("crash child manifest should remain durable");
    let desired = desired_policy();
    assert_eq!(
        before.spec.egress, desired,
        "pre-effect desired policy must survive the killed acknowledgement window"
    );
    assert_eq!(before.egress_policy_reload.desired_generation().get(), 2);
    assert_eq!(before.egress_policy_reload.latest_attempt_generation(), 1);
    assert!(
        before.egress_policy_reload.is_applying(),
        "crash before completion publication must retain Applying"
    );

    let assignment = before
        .egress_proxy
        .as_ref()
        .expect("crash manifest should retain the exact PEP assignment");
    let authority = LocalPortLeaseAuthority::open(&config.state_root)
        .expect("fresh recovery should reopen the shared port authority");
    let dead_owner = authority
        .inspect(assignment.port_lease.lease_id())
        .expect("fresh recovery should inspect the prior PEP lease")
        .expect("prior PEP lease should remain durable");
    assert_eq!(dead_owner.phase(), PortLeasePhase::Active);
    let dead_lifetime = dead_owner
        .active_lifetime()
        .expect("crash-owned PEP should retain exact lifetime evidence");
    assert_eq!(
        dead_lifetime.effect_scope(),
        PortLeaseEffectScope::ProcessBound
    );
    assert_eq!(dead_lifetime.generation().as_u64(), 1);

    backend
        .reload_egress_policy(&sandbox_id, desired.clone())
        .expect("fresh process should reconcile the exact durable reload attempt");
    let completed = backend
        .read_manifest(&sandbox_id)
        .expect("completed manifest should read")
        .expect("completed manifest should remain");
    assert_eq!(completed.spec.egress, desired);
    assert_eq!(completed.egress_policy_reload.desired_generation().get(), 2);
    assert_eq!(
        completed.egress_policy_reload.latest_attempt_generation(),
        1
    );
    assert!(
        !completed.egress_policy_reload.is_applying(),
        "exact fresh-process reconciliation must publish Stable"
    );

    let rebound = authority
        .inspect(assignment.port_lease.lease_id())
        .expect("rebound PEP lease should inspect")
        .expect("rebound PEP lease should remain durable");
    assert_eq!(rebound.phase(), PortLeasePhase::Active);
    let rebound_lifetime = rebound
        .active_lifetime()
        .expect("fresh PEP should retain its process lifetime");
    assert_eq!(
        rebound_lifetime.generation().as_u64(),
        dead_lifetime.generation().as_u64() + 1,
        "one fresh owner generation should replace the killed PEP"
    );

    let readiness = backend
        .egress_proxies
        .readiness(&completed.spec.tenant_id, &sandbox_id)
        .expect("fresh PEP readiness should inspect")
        .expect("fresh PEP should be registered");
    assert_eq!(
        readiness
            .policy_generation
            .map(|generation| generation.get()),
        Some(2),
        "the fresh PEP should tag the exact durable attempt once"
    );
    let stable_bytes =
        std::fs::read(&completed.conmon_layout.manifest_path).expect("stable bytes should read");
    backend
        .reload_egress_policy(&sandbox_id, desired)
        .expect("stable exact replay should be idempotent");
    let replay_readiness = backend
        .egress_proxies
        .readiness(&completed.spec.tenant_id, &sandbox_id)
        .expect("replayed PEP readiness should inspect")
        .expect("replayed PEP should remain registered");
    assert_eq!(
        replay_readiness.policy_generation, readiness.policy_generation,
        "stable replay must not apply the provider attempt twice"
    );
    assert_eq!(
        std::fs::read(&completed.conmon_layout.manifest_path)
            .expect("replayed stable bytes should read"),
        stable_bytes,
        "stable replay must not rewrite canonical desired or attempt state"
    );

    println!("{RECOVERED_OBSERVATION}");
}

struct ChildOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn spawn_child(test_name: &str, root: &Path, port: u16) -> Child {
    Command::new(std::env::current_exe().expect("current test executable should resolve"))
        .arg("--exact")
        .arg(test_name)
        .arg("--ignored")
        .arg("--nocapture")
        .env(ROOT_ENV, root)
        .env(PORT_ENV, port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("failed to spawn child test {test_name}: {error}"))
}

fn run_child_to_completion(test_name: &str, root: &Path, port: u16) -> ChildOutput {
    let mut child = spawn_child(test_name, root, port);
    let deadline = Instant::now() + CHILD_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
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

fn terminate_child(child: &mut Child) {
    match child.kill() {
        Ok(()) => {}
        Err(error) if matches!(error.kind(), std::io::ErrorKind::InvalidInput) => {}
        Err(error) => panic!("failed to terminate child {}: {error}", child.id()),
    }
}

fn child_config() -> (PathBuf, u16) {
    let root = PathBuf::from(std::env::var(ROOT_ENV).expect("shared crash root should be set"));
    let port = std::env::var(PORT_ENV)
        .expect("shared PEP port should be set")
        .parse()
        .expect("shared PEP port should be numeric");
    (root, port)
}

fn backend_config(root: &Path, port: u16) -> ContainerSandboxBackendConfig {
    let mut config = ContainerSandboxBackendConfig::under_root(root);
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = port..=port;
    config
}

fn desired_policy() -> EgressPolicy {
    EgressPolicy::new([EgressRule::new(
        "fresh-process-reload",
        EgressProtocol::Https,
        "example.com",
        443,
    )])
}

fn unused_loopback_port() -> u16 {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .expect("test-only port picker should bind")
        .local_addr()
        .expect("test-only port picker should inspect")
        .port()
}
