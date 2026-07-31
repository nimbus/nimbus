//! Fresh-process crash proof for acknowledged egress-policy reloads.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use nimbus_egress::{EgressPolicy, EgressProtocol, EgressRule};
use nimbus_network::{LocalPortLeaseAuthority, PortLeaseEffectScope, PortLeasePhase};
use tempfile::TempDir;

use super::support::*;
use super::*;
use crate::backends::oci::command::CommandSpec;
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
fn container_ready_rejects_active_pep_for_prior_desired_policy_attempt() {
    let root = TempDir::new().expect("stale-policy state root should exist");
    let pep_port = unused_loopback_port();
    let backend = ContainerSandboxBackend::new(backend_config(root.path(), pep_port));
    let sandbox_id = SandboxId::new("container-stale-egress-policy");
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id, None, None)
        .expect("stale-policy fixture should reserve exact network authority")
        .manifest;
    let launch_claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("fresh execute plan should retain its launch claim");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&launch_claim),
        )
        .expect("generation-1 PEP should start");
    manifest.launch_reservation_claim = None;
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' '{{\"id\":\"{}\",\"status\":\"running\"}}'",
            manifest.handle.id
        ),
    ]);

    let prior_policy_readiness = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &sandbox_id)
        .expect("generation-1 PEP readiness should inspect")
        .expect("generation-1 PEP should remain registered");
    assert!(prior_policy_readiness.is_ready());
    assert!(prior_policy_readiness.audit_healthy());
    assert_eq!(
        prior_policy_readiness
            .policy_generation()
            .map(|generation| generation.get()),
        Some(1),
        "fixture must retain the initial active policy generation"
    );

    let pending_attempt = manifest
        .egress_policy_reload
        .begin()
        .expect("desired generation 2 should begin");
    manifest.spec.egress = desired_policy();
    backend
        .write_manifest(&manifest)
        .expect("desired generation 2 Applying state should be durable before provider effect");
    assert_eq!(manifest.egress_policy_reload.desired_generation().get(), 2);
    assert_eq!(manifest.egress_policy_reload.latest_attempt_generation(), 1);
    assert_eq!(
        manifest
            .egress_policy_reload
            .pending_attempt()
            .expect("Applying state should expose its exact pending attempt"),
        Some(pending_attempt)
    );
    assert!(manifest.egress_policy_reload.is_applying());

    let still_prior = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &sandbox_id)
        .expect("prior-policy PEP readiness should remain inspectable")
        .expect("prior-policy PEP should remain registered");
    assert_eq!(
        still_prior.policy_generation(),
        prior_policy_readiness.policy_generation(),
        "persisting desired generation 2 must not mutate the generation-1 PEP"
    );
    let launch_error = backend
        .require_authenticated_egress_readiness(&manifest)
        .expect_err("the exact pre-spawn gate must reject the stale PEP dependency");
    assert!(
        launch_error.to_string().contains("denied launch")
            && launch_error
                .to_string()
                .contains("egress PEP dependency is not ready"),
        "the pre-spawn gate must fail before any runtime creator effect: {launch_error}"
    );

    let observed = backend
        .inspect_sync(&sandbox_id)
        .expect("running stale-policy fixture should remain inspectable")
        .expect("running stale-policy fixture should remain visible");
    assert!(
        observed.published_endpoints.is_empty(),
        "stale PEP policy evidence must withdraw published endpoints"
    );
    assert_eq!(
        observed.status,
        SandboxStatus::NotReady,
        "a running container must not report Ready while its PEP realizes the prior desired policy"
    );

    backend
        .reload_egress_policy(&sandbox_id, desired_policy())
        .expect("exact current attachment must permit the stale policy bytes to reconcile");
    let completed = backend
        .read_manifest(&sandbox_id)
        .expect("completed stale-policy manifest should inspect")
        .expect("completed stale-policy manifest should remain");
    assert!(
        !completed.egress_policy_reload.is_applying(),
        "exact reconciliation should complete the durable attempt"
    );
    backend
        .require_authenticated_egress_readiness(&completed)
        .expect("exact reconciled attachment should become ready");
    let reconciled = backend
        .egress_proxies
        .readiness(&completed.spec.tenant_id, &sandbox_id)
        .expect("reconciled PEP should inspect")
        .expect("reconciled PEP should remain registered");
    assert_eq!(
        reconciled
            .policy_generation()
            .map(|generation| generation.get()),
        Some(2),
        "the exact durable attempt should advance the stale PEP exactly once"
    );
}

#[test]
fn stable_reload_reseeds_exact_attempt_after_pep_process_replacement() {
    let root = TempDir::new().expect("replacement state root should exist");
    let pep_port = unused_loopback_port();
    let config = backend_config(root.path(), pep_port);
    let backend = ContainerSandboxBackend::new(config.clone());
    let sandbox_id = SandboxId::new("stable-reload-pep-replacement");
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id, None, None)
        .expect("replacement fixture should reserve exact network authority")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("replacement baseline manifest should publish");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("fresh execute plan should retain its reservation claim"),
            ),
        )
        .expect("replacement baseline PEP should start");
    manifest.launch_reservation_claim = None;
    backend
        .write_manifest(&manifest)
        .expect("replacement running manifest should publish");

    let desired = desired_policy();
    backend
        .reload_egress_policy(&sandbox_id, desired.clone())
        .expect("first exact reload should complete");
    let completed = backend
        .read_manifest(&sandbox_id)
        .expect("stable manifest should inspect")
        .expect("stable manifest should remain");
    assert!(!completed.egress_policy_reload.is_applying());
    assert_eq!(completed.egress_policy_reload.desired_generation().get(), 2);
    assert_eq!(
        completed.egress_policy_reload.latest_attempt_generation(),
        1
    );
    let first = backend
        .egress_proxies
        .readiness(&completed.spec.tenant_id, &sandbox_id)
        .expect("first PEP should inspect")
        .expect("first PEP should remain registered");
    assert_eq!(
        first.policy_generation().map(|generation| generation.get()),
        Some(2),
        "the completed attempt should advance the original PEP exactly once"
    );
    drop(backend);

    let replacement = ContainerSandboxBackend::new(config);
    replacement
        .ensure_egress_proxy_running_with_release_authority(
            &completed,
            PepPreAdoptionReleaseAuthority::Retain,
        )
        .expect("release-authority reconstruction should seed the replacement PEP");
    let replaced = replacement
        .egress_proxies
        .readiness(&completed.spec.tenant_id, &sandbox_id)
        .expect("replacement PEP should inspect")
        .expect("replacement PEP should be registered");
    assert_eq!(
        replaced
            .policy_generation()
            .map(|generation| generation.get()),
        Some(2),
        "replacement PEP must replay the exact durable attempt instead of remaining untagged"
    );
    replacement
        .require_authenticated_egress_readiness(&completed)
        .expect("replacement PEP should be launch-ready without a reload API call");

    replacement
        .reload_egress_policy(&sandbox_id, desired)
        .expect("stable exact replacement replay should be idempotent");
    let replayed = replacement
        .egress_proxies
        .readiness(&completed.spec.tenant_id, &sandbox_id)
        .expect("replayed replacement PEP should inspect")
        .expect("replayed replacement PEP should remain registered");
    assert_eq!(
        replayed.policy_generation(),
        replaced.policy_generation(),
        "stable replay must not duplicate the provider generation"
    );
}

#[test]
fn applying_reload_rejects_foreign_listener_before_policy_or_manifest_completion() {
    let root = TempDir::new().expect("foreign-listener state root should exist");
    let pep_port = unused_loopback_port();
    let backend = ContainerSandboxBackend::new(backend_config(root.path(), pep_port));
    let sandbox_id = SandboxId::new("reload-foreign-listener");
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id, None, None)
        .expect("foreign-listener fixture should reserve exact network authority")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("foreign-listener baseline manifest should publish");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("fresh execute plan should retain its reservation claim"),
            ),
        )
        .expect("foreign-listener baseline PEP should start");
    manifest.launch_reservation_claim = None;
    backend
        .write_manifest(&manifest)
        .expect("foreign-listener running manifest should publish");
    let policy_before = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &sandbox_id)
        .expect("baseline PEP should inspect")
        .expect("baseline PEP should remain registered");
    assert_eq!(
        policy_before
            .policy_generation()
            .map(|generation| generation.get()),
        Some(1)
    );

    manifest
        .egress_policy_reload
        .begin()
        .expect("foreign-listener Applying attempt should begin");
    let desired = desired_policy();
    manifest.spec.egress = desired.clone();
    manifest
        .egress_proxy
        .as_mut()
        .expect("execute manifest should retain its PEP assignment")
        .host = Ipv4Addr::new(127, 0, 0, 2).to_string();
    backend
        .write_manifest(&manifest)
        .expect("foreign-listener Applying state should publish");
    let applying_bytes = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("Applying manifest bytes should read");

    let error = backend
        .reload_egress_policy(&sandbox_id, desired)
        .expect_err("foreign listener evidence must reject durable reload reconciliation");
    assert!(
        error.to_string().contains("listener")
            || error.to_string().contains("attachment")
            || error.to_string().contains("authority"),
        "rejection must identify the unauthenticated lifecycle attachment: {error}"
    );
    let policy_after = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &sandbox_id)
        .expect("rejected PEP should remain inspectable")
        .expect("rejected PEP should remain registered");
    assert_eq!(
        policy_after, policy_before,
        "attachment rejection must precede every PEP policy mutation"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("rejected Applying manifest bytes should read"),
        applying_bytes,
        "attachment rejection must not complete or otherwise rewrite durable reload state"
    );
    let persisted = backend
        .read_manifest(&sandbox_id)
        .expect("rejected manifest should inspect")
        .expect("rejected manifest should remain");
    assert!(
        persisted.egress_policy_reload.is_applying(),
        "unauthenticated registration must not complete the durable reload attempt"
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
    let authority = LocalPortLeaseAuthority::open(&config.network_state_root)
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
            .policy_generation()
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
        replay_readiness.policy_generation(),
        readiness.policy_generation(),
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

#[test]
fn reload_egress_policy_updates_running_container_proxy() {
    let first = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nfirst");
    let second = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nsecond");
    let temp_dir = TempDir::new().expect("tempdir should build");
    let proxy_port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = proxy_port..=proxy_port;
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_egress_policy(allow_loopback_http_policy(first.addr.port())),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("manifest should persist before reload");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("execute plan should retain launch claim"),
            ),
        )
        .expect("egress proxy should start on loopback test subnet");
    manifest.launch_reservation_claim = None;
    backend
        .write_manifest(&manifest)
        .expect("running manifest should publish post-launch authority");
    let proxy_addr = manifest
        .egress_proxy
        .as_ref()
        .expect("proxy assignment should exist")
        .bind_addr()
        .expect("proxy bind address should parse");

    let allowed_first = proxy_request(
        proxy_addr,
        format!(
            "GET http://127.0.0.1:{}/ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            first.addr.port()
        ),
    );
    assert!(
        allowed_first.starts_with("HTTP/1.1 200 OK") && allowed_first.contains("first"),
        "initial policy should allow first upstream, got: {allowed_first}"
    );

    backend
        .reload_egress_policy(
            &manifest.handle.id,
            allow_loopback_http_policy(second.addr.port()),
        )
        .expect("egress policy reload should update live proxy");
    let denied_old = proxy_request(
        proxy_addr,
        format!(
            "GET http://127.0.0.1:{}/ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            first.addr.port()
        ),
    );
    let allowed_new = proxy_request(
        proxy_addr,
        format!(
            "GET http://127.0.0.1:{}/ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            second.addr.port()
        ),
    );

    assert!(
        denied_old.starts_with("HTTP/1.1 403 Forbidden"),
        "old upstream should be denied after reload, got: {denied_old}"
    );
    assert!(
        allowed_new.starts_with("HTTP/1.1 200 OK") && allowed_new.contains("second"),
        "new upstream should be allowed after reload, got: {allowed_new}"
    );
    let reloaded_manifest = backend
        .read_manifest(&manifest.handle.id)
        .expect("manifest read should succeed")
        .expect("manifest should remain");
    assert_eq!(
        reloaded_manifest.spec.egress.rules()[0].port,
        second.addr.port()
    );
}

fn allow_loopback_http_policy(port: u16) -> EgressPolicy {
    EgressPolicy::new([
        EgressRule::new("loopback-test", EgressProtocol::Http, "127.0.0.1", port)
            .allow_internal_ips(true),
    ])
}

fn proxy_request(proxy_addr: SocketAddr, request: String) -> String {
    let mut stream = TcpStream::connect(proxy_addr).expect("client should connect to proxy");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should set");
    stream
        .write_all(request.as_bytes())
        .expect("client should write request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("client should read response");
    response
}

struct TestHttpServer {
    addr: SocketAddr,
}

impl TestHttpServer {
    fn start(response: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
        thread::spawn(move || {
            while let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self { addr }
    }
}

fn unused_loopback_port() -> u16 {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .expect("test-only port picker should bind")
        .local_addr()
        .expect("test-only port picker should inspect")
        .port()
}
