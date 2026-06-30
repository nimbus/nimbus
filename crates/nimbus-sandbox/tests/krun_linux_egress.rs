#![cfg(target_os = "linux")]

//! KME2 runtime deny proof for the krun (libkrun/TSI microVM) backend.
//!
//! The krun execute path runs its VMM inside a deny-by-default network
//! namespace. Under libkrun TSI the guest's `connect()`/`sendto()` are host
//! sockets issued by the VMM process, so a netns around the VMM confines the
//! guest's egress to the namespace's only outbound path: a host-side egress PEP
//! bound on the bridge gateway. This test proves that a guest attempting direct
//! external egress with no reachable proxy gets a route failure.
//!
//! It is gated behind both `#[ignore]` and a `/dev/kvm` precondition. The krun
//! execute path is still fail-closed before launch planning (lifted by KME4),
//! so until KME4 lands this proof cannot boot a guest; it exists now as the
//! pinned runtime harness and runs once execute mode is enabled on real KVM
//! hardware as root.
//!
//! Teardown never relies on guest exit: a one-shot TSI guest that holds an
//! ESTABLISHED connection hangs VM teardown, so the harness kills the VMM and
//! releases the persistent netns under hard timeouts.

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use tempfile::TempDir;

use nimbus_core::TenantId;
use nimbus_egress::{EgressPolicy, EgressProtocol, EgressRule};
use nimbus_sandbox::backends::container::{
    ContainerSandboxBackend, ContainerSandboxBackendConfig, ContainerStartMode,
};
use nimbus_sandbox::backends::krun::{KrunSandboxBackend, KrunSandboxBackendConfig, KrunStartMode};
use nimbus_sandbox::{
    SandboxBackend, SandboxBackendKind, SandboxId, SandboxMountSpec, SandboxOwnerSpec,
    SandboxProcessSpec, SandboxRootSpec, SandboxSpec,
};

const RESULT_VOLUME: &str = "krun-egress-proof";
const RESULT_PATH_IN_GUEST: &str = "/nimbus-egress/result";
const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const FORCE_CMD_TIMEOUT_SECS: u64 = 10;

#[test]
#[ignore = "requires a Linux root host with /dev/kvm, crun(krun), conmon, buildah, netavark, aardvark-dns, and OCI image pull; the runtime deny proof is gated behind the KME4 execute fail-close lift"]
fn krun_execute_mode_denies_direct_external_egress() {
    if !egress_proof_preconditions_met() {
        return;
    }

    let (temp_dir, workdir) = test_workdir("krun-direct-egress");

    let backend = KrunSandboxBackend::new(egress_backend_config(&workdir, 15300));
    let tenant_id = TenantId::new("krun-egress-proof").expect("tenant id should parse");
    let image = env::var("NIMBUS_KRUN_EGRESS_IMAGE")
        .unwrap_or_else(|_| "docker://busybox:latest".to_owned());
    let target =
        env::var("NIMBUS_KRUN_EGRESS_TARGET").unwrap_or_else(|_| "http://1.1.1.1".to_owned());
    let command = format!(
        "if (unset HTTP_PROXY http_proxy HTTPS_PROXY https_proxy ALL_PROXY all_proxy NO_PROXY no_proxy; wget -T 4 -q -O /tmp/nimbus-egress-body {target:?}); then echo allowed > {RESULT_PATH_IN_GUEST}; else echo denied > {RESULT_PATH_IN_GUEST}; fi; sleep 30"
    );
    let spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::standalone_named("krun-deny-direct-egress"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command([command]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ));

    let handle = block_on(backend.start(spec))
        .expect("krun guest should start once execute mode is enabled (KME4)");

    // The guard force-tears-down on scope exit AND on panic: it never blocks on
    // guest exit, killing the VMM and releasing the netns under hard timeouts.
    let _teardown = ForceTeardownGuard::new(
        backend.clone(),
        handle.id.clone(),
        sandbox_netns_path(&workdir, &tenant_id, &handle.id),
    );

    let result_path = tenant_volume_path(&workdir, &tenant_id, RESULT_VOLUME).join("result");
    let result = wait_for_result(
        &result_path,
        Duration::from_secs(25),
        "krun direct-egress proof result",
    );

    assert_eq!(
        result.trim(),
        "denied",
        "a krun guest with no reachable egress proxy must get a route failure inside the deny-by-default network namespace"
    );

    drop(temp_dir);
}

/// Cross-substrate parity proof: ONE egress policy enforced through the krun PEP
/// and the container PEP must yield byte-identical allow/deny. The policy allows
/// a single internal upstream and denies everything else (`evil.example` and a
/// second tenant's endpoint, `cross_tenant_reach`).
///
/// Gated behind `#[ignore]` + `/dev/kvm`: the krun execute path is still
/// fail-closed before launch planning (lifted by KME4), so the krun half cannot
/// boot a guest until then. The container half runs today; this is the pinned
/// parity harness that turns green once execute mode is enabled on real KVM
/// hardware as root.
#[test]
#[ignore = "requires a Linux root host with /dev/kvm plus the full krun + container OCI runtime stack; the krun half is gated behind the KME4 execute fail-close lift"]
fn krun_and_container_pep_enforce_identical_allow_deny() {
    if !egress_proof_preconditions_met() {
        return;
    }

    // One allowed internal upstream plus one disallowed upstream that stands in
    // for both `evil.example` and a second tenant's endpoint.
    let allowed = TestHttpServer::start("allowed-body");
    let cross_tenant = TestHttpServer::start("cross-tenant-body");

    // ONE policy drives both substrates.
    let policy = parity_policy(allowed.addr.port());

    let krun_result = run_krun_parity_probe(&policy, allowed.addr.port(), cross_tenant.addr.port());
    let container_result =
        run_container_parity_probe(&policy, allowed.addr.port(), cross_tenant.addr.port());

    // Byte-identical allow/deny across the two PEPs is the parity guarantee.
    assert_eq!(
        krun_result, container_result,
        "krun PEP and container PEP must yield byte-identical allow/deny for one policy"
    );
    assert_result_line(&krun_result, "allowed_internal=allowed");
    assert_result_line(&krun_result, "evil_denied=denied");
    assert_result_line(&krun_result, "cross_tenant_reach=denied");
}

fn run_krun_parity_probe(
    policy: &EgressPolicy,
    allowed_port: u16,
    cross_tenant_port: u16,
) -> String {
    let (_temp_dir, workdir) = test_workdir("krun-parity");
    let backend = KrunSandboxBackend::new(egress_backend_config(&workdir, 15400));
    let tenant_id = TenantId::new("krun-parity-tenant").expect("tenant id should parse");
    let image = env::var("NIMBUS_KRUN_EGRESS_IMAGE")
        .unwrap_or_else(|_| "docker://busybox:latest".to_owned());
    let spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::standalone_named("krun-parity-probe"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command([parity_probe_command(allowed_port, cross_tenant_port)]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ))
    .with_egress_policy(policy.clone());

    let handle = block_on(backend.start(spec))
        .expect("krun guest should start once execute mode is enabled (KME4)");
    let teardown = ForceTeardownGuard::new(
        backend.clone(),
        handle.id.clone(),
        sandbox_netns_path(&workdir, &tenant_id, &handle.id),
    );

    let result_path = tenant_volume_path(&workdir, &tenant_id, RESULT_VOLUME).join("result");
    let result = wait_for_result(
        &result_path,
        Duration::from_secs(25),
        "krun parity probe result",
    );
    // Force-tear-down the guest while the workdir still backs the netns.
    drop(teardown);
    normalize_result(&result)
}

fn run_container_parity_probe(
    policy: &EgressPolicy,
    allowed_port: u16,
    cross_tenant_port: u16,
) -> String {
    let (_temp_dir, workdir) = test_workdir("container-parity");
    let backend = ContainerSandboxBackend::new(container_parity_config(&workdir, 15500));
    let tenant_id = TenantId::new("container-parity-tenant").expect("tenant id should parse");
    let image = env::var("NIMBUS_CONTAINER_EGRESS_IMAGE")
        .unwrap_or_else(|_| "docker.io/library/busybox:latest".to_owned());
    let spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::standalone_named("container-parity-probe"),
        SandboxBackendKind::Container,
        SandboxRootSpec::oci_image_reference(image),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command([parity_probe_command(allowed_port, cross_tenant_port)]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ))
    .with_egress_policy(policy.clone());

    let handle = block_on(backend.start(spec)).expect("container should start");
    let result_path = tenant_volume_path(&workdir, &tenant_id, RESULT_VOLUME).join("result");
    let result = wait_for_result(
        &result_path,
        Duration::from_secs(20),
        "container parity probe result",
    );
    let _ = block_on(backend.stop(&handle.id));
    let _ = block_on(backend.remove_tenant_artifacts(tenant_id));
    normalize_result(&result)
}

fn parity_policy(allowed_port: u16) -> EgressPolicy {
    EgressPolicy::new([EgressRule::new(
        "parity-allowed-internal",
        EgressProtocol::Http,
        "127.0.0.1",
        allowed_port,
    )
    .with_methods(["GET"])
    .with_path_prefixes(["/allowed"])
    .allow_internal_ips(true)])
}

fn parity_probe_command(allowed_port: u16, cross_tenant_port: u16) -> String {
    format!(
        r#"TMP=/nimbus-egress/result.tmp
: > "$TMP"
if wget -T 5 -q -O /tmp/allowed "http://127.0.0.1:{allowed_port}/allowed" && grep -q allowed-body /tmp/allowed; then echo allowed_internal=allowed >> "$TMP"; else echo allowed_internal=denied >> "$TMP"; fi
if wget -T 5 -q -O /tmp/evil "http://evil.example/"; then echo evil_denied=allowed >> "$TMP"; else echo evil_denied=denied >> "$TMP"; fi
if wget -T 5 -q -O /tmp/cross "http://127.0.0.1:{cross_tenant_port}/allowed"; then echo cross_tenant_reach=allowed >> "$TMP"; else echo cross_tenant_reach=denied >> "$TMP"; fi
mv "$TMP" {RESULT_PATH_IN_GUEST}
sleep 30"#
    )
}

fn normalize_result(result: &str) -> String {
    let mut lines: Vec<&str> = result
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    lines.sort_unstable();
    lines.join("\n")
}

fn assert_result_line(results: &str, expected: &str) {
    assert!(
        results.lines().any(|line| line == expected),
        "expected result line {expected:?}, got:\n{results}"
    );
}

fn container_parity_config(workdir: &Path, first_port: u16) -> ContainerSandboxBackendConfig {
    let mut config = ContainerSandboxBackendConfig::under_root(workdir);
    config.start_mode = ContainerStartMode::Execute;
    config.conmon_path = default_existing_path("/usr/bin/conmon", "conmon");
    config.runtime_path = default_existing_path("/usr/bin/crun", "crun");
    config.buildah_path = default_existing_path("/usr/bin/buildah", "buildah");
    config.netavark_path = env::var_os("NIMBUS_NETAVARK")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/lib/podman/netavark", "netavark"));
    config.aardvark_dns_path = env::var_os("NIMBUS_AARDVARK_DNS")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/lib/podman/aardvark-dns", "aardvark-dns"));
    config.use_buildah_unshare = false;
    config.published_port_range = first_port..=first_port + 20;
    config.start_timeout = Duration::from_secs(30);
    config
}

struct TestHttpServer {
    addr: SocketAddr,
}

impl TestHttpServer {
    fn start(body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("parity upstream should bind");
        let addr = listener
            .local_addr()
            .expect("parity upstream address should resolve");
        thread::spawn(move || {
            while let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self { addr }
    }
}

fn egress_proof_preconditions_met() -> bool {
    // SAFETY: geteuid has no preconditions and does not dereference pointers.
    let is_root = unsafe { libc::geteuid() } == 0;
    if !is_root {
        eprintln!(
            "skipping krun egress proof: must run as root to create persistent network namespaces"
        );
        return false;
    }
    if !Path::new("/dev/kvm").exists() {
        eprintln!("skipping krun egress proof: /dev/kvm is required to boot a libkrun microVM");
        return false;
    }
    true
}

fn egress_backend_config(workdir: &Path, first_port: u16) -> KrunSandboxBackendConfig {
    let mut config = KrunSandboxBackendConfig::under_root(workdir);
    config.start_mode = KrunStartMode::Execute;
    config.runtime_path = env::var_os("NIMBUS_KRUN_EGRESS_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/libexec/nimbus/crun", "crun"));
    config.conmon_path = env::var_os("NIMBUS_KRUN_EGRESS_CONMON")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/bin/conmon", "conmon"));
    config.buildah_path = env::var_os("NIMBUS_KRUN_EGRESS_BUILDAH")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/bin/buildah", "buildah"));
    config.netavark_path = env::var_os("NIMBUS_NETAVARK")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/lib/podman/netavark", "netavark"));
    config.aardvark_dns_path = env::var_os("NIMBUS_AARDVARK_DNS")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_existing_path("/usr/lib/podman/aardvark-dns", "aardvark-dns"));
    config.use_buildah_unshare = false;
    config.published_port_range = first_port..=first_port + 20;
    config.start_timeout = Duration::from_secs(30);
    config
}

fn test_workdir(name: &str) -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().expect("temporary workdir should be created");
    let workdir = env::var_os("NIMBUS_KRUN_EGRESS_WORKDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| temp_dir.path().to_path_buf())
        .join(name);
    fs::create_dir_all(&workdir).expect("egress proof workdir should exist");
    (temp_dir, workdir)
}

fn default_existing_path(preferred: &str, fallback: &str) -> PathBuf {
    let preferred = PathBuf::from(preferred);
    if preferred.exists() {
        preferred
    } else {
        PathBuf::from(fallback)
    }
}

fn tenant_volume_path(workdir: &Path, tenant_id: &TenantId, volume_name: &str) -> PathBuf {
    workdir
        .join("state")
        .join("tenants")
        .join(tenant_id.as_str())
        .join("volumes")
        .join(volume_name)
}

fn sandbox_netns_path(workdir: &Path, tenant_id: &TenantId, sandbox_id: &SandboxId) -> PathBuf {
    workdir
        .join("state")
        .join("tenants")
        .join(tenant_id.as_str())
        .join("networks")
        .join("netns")
        .join(sandbox_id.as_str())
}

fn wait_for_result(path: &Path, timeout: Duration, label: &str) -> String {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(contents) = fs::read_to_string(path) {
            return contents;
        }
        thread::sleep(Duration::from_millis(200));
    }
    panic!("timed out waiting for {label} at {}", path.display());
}

/// Force-teardown harness: stop gracefully on a worker thread but never block on
/// guest exit, then kill the VMM and release the persistent netns under hard
/// timeouts. Runs on both the happy path (scope exit) and the panic path.
struct ForceTeardownGuard {
    backend: KrunSandboxBackend,
    sandbox_id: SandboxId,
    netns_path: PathBuf,
}

impl ForceTeardownGuard {
    fn new(backend: KrunSandboxBackend, sandbox_id: SandboxId, netns_path: PathBuf) -> Self {
        Self {
            backend,
            sandbox_id,
            netns_path,
        }
    }
}

impl Drop for ForceTeardownGuard {
    fn drop(&mut self) {
        let stop_handle = {
            let backend = self.backend.clone();
            let sandbox_id = self.sandbox_id.clone();
            thread::spawn(move || {
                let _ = block_on(backend.stop(&sandbox_id));
            })
        };

        let deadline = Instant::now() + GRACEFUL_STOP_TIMEOUT;
        while Instant::now() < deadline && !stop_handle.is_finished() {
            thread::sleep(Duration::from_millis(100));
        }

        if !stop_handle.is_finished() {
            // A one-shot TSI guest holding an ESTABLISHED connection hangs VM
            // teardown; kill the VMM (conmon/crun/krun) hard so the persistent
            // netns can be released.
            run_with_timeout("pkill", &["-9", "-f", self.sandbox_id.as_str()]);
        }

        // Release the persistent netns bind-mount regardless of guest state.
        run_with_timeout("umount", &["-l", &self.netns_path.to_string_lossy()]);
        let _ = fs::remove_file(&self.netns_path);
        let _ = stop_handle.join();
    }
}

fn run_with_timeout(program: &str, args: &[&str]) {
    let _ = Command::new("timeout")
        .arg(FORCE_CMD_TIMEOUT_SECS.to_string())
        .arg(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
