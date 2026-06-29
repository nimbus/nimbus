#![cfg(target_os = "linux")]

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use tempfile::TempDir;

use nimbus_core::TenantId;
use nimbus_egress::{EgressPolicy, EgressProtocol, EgressRule};
use nimbus_sandbox::backends::container::{
    ContainerSandboxBackend, ContainerSandboxBackendConfig, ContainerStartMode,
};
use nimbus_sandbox::{
    SandboxBackend, SandboxBackendKind, SandboxId, SandboxMountSpec, SandboxOwnerSpec,
    SandboxProcessSpec, SandboxRootSpec, SandboxSpec,
};

const RESULT_VOLUME: &str = "egress-proof";
const RESULT_PATH_IN_GUEST: &str = "/nimbus-egress/result";
const PHASE1_PATH_IN_GUEST: &str = "/nimbus-egress/phase1";
const PHASE2_TRIGGER_IN_GUEST: &str = "/nimbus-egress/phase2-go";
const PHASE2_PATH_IN_GUEST: &str = "/nimbus-egress/phase2";

#[test]
#[ignore = "requires Linux root with conmon, crun, buildah, netavark, aardvark-dns, and OCI image pull access"]
fn container_execute_mode_denies_direct_external_egress() {
    assert_root();

    let (temp_dir, workdir) = test_workdir("direct-egress");

    let backend = ContainerSandboxBackend::new(smoke_config(&workdir, 15100));
    let tenant_id = TenantId::new("egress-proof").expect("tenant id should parse");
    let image = env::var("NIMBUS_CONTAINER_EGRESS_IMAGE")
        .unwrap_or_else(|_| "docker.io/library/busybox:latest".to_owned());
    let target =
        env::var("NIMBUS_CONTAINER_EGRESS_TARGET").unwrap_or_else(|_| "http://1.1.1.1".to_owned());
    let command = format!(
        "if (unset HTTP_PROXY http_proxy HTTPS_PROXY https_proxy ALL_PROXY all_proxy NO_PROXY no_proxy; wget -T 4 -q -O /tmp/nimbus-egress-body {target:?}); then echo allowed > {RESULT_PATH_IN_GUEST}; else echo denied > {RESULT_PATH_IN_GUEST}; fi; sleep 30"
    );
    let spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::standalone_named("deny-direct-egress"),
        SandboxBackendKind::Container,
        SandboxRootSpec::oci_image_reference(image.clone()),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command([command]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ));
    let handle = block_on(backend.start(spec)).expect("container should start");
    let cleanup = CleanupGuard::new(backend.clone(), handle.id.clone(), tenant_id.clone());

    let result_path = tenant_volume_path(&workdir, &tenant_id, RESULT_VOLUME).join("result");
    let result = wait_for_result(
        &result_path,
        Duration::from_secs(20),
        "direct-egress proof result",
    );

    block_on(backend.stop(&handle.id))
        .expect("container should stop and release network artifacts");
    block_on(backend.remove_tenant_artifacts(tenant_id))
        .expect("tenant artifacts should clean up after proof");
    cleanup.disarm();

    assert_eq!(
        result.trim(),
        "denied",
        "direct guest egress should be denied by the netavark internal bridge"
    );
    drop(temp_dir);
}

#[test]
#[ignore = "requires Linux root with conmon, crun, buildah, netavark, aardvark-dns, and OCI image pull access"]
fn container_execute_mode_enforces_proxy_policy_and_live_reload() {
    assert_root();

    let (temp_dir, workdir) = test_workdir("proxy-matrix");
    let phase_one_upstream = TestHttpServer::start("allowed-v1");
    let phase_two_upstream = TestHttpServer::start("allowed-v2");

    let backend = ContainerSandboxBackend::new(smoke_config(&workdir, 15200));
    let tenant_id = TenantId::new("egress-matrix").expect("tenant id should parse");
    let image = env::var("NIMBUS_CONTAINER_EGRESS_IMAGE")
        .unwrap_or_else(|_| "docker.io/library/busybox:latest".to_owned());
    let direct_target =
        env::var("NIMBUS_CONTAINER_EGRESS_TARGET").unwrap_or_else(|_| "http://1.1.1.1".to_owned());
    let command = proxy_matrix_command(
        phase_one_upstream.addr.port(),
        phase_two_upstream.addr.port(),
        &direct_target,
    );
    let spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::standalone_named("proxy-egress-matrix"),
        SandboxBackendKind::Container,
        SandboxRootSpec::oci_image_reference(image.clone()),
        SandboxProcessSpec::new(Vec::<String>::new())
            .with_entrypoint(["/bin/sh", "-c"])
            .with_command([command]),
    )
    .with_mount(SandboxMountSpec::tenant_volume(
        RESULT_VOLUME,
        "/nimbus-egress",
    ))
    .with_egress_policy(phase_one_policy(phase_one_upstream.addr.port()));
    let handle = block_on(backend.start(spec)).expect("container should start");
    let cleanup = CleanupGuard::new(backend.clone(), handle.id.clone(), tenant_id.clone());
    let volume = tenant_volume_path(&workdir, &tenant_id, RESULT_VOLUME);

    let phase1 = wait_for_result(
        &volume.join("phase1"),
        Duration::from_secs(45),
        "phase-one proxy conformance result",
    );
    assert_result_line(&phase1, "allowed_v1=allowed");
    assert_result_line(&phase1, "l7_path_denied=denied");
    assert_result_line(&phase1, "loopback_default_denied=denied");
    assert_result_line(&phase1, "direct_bypass=denied");

    backend
        .reload_egress_policy(
            &handle.id,
            phase_two_policy(
                phase_two_upstream.addr.port(),
                phase_one_upstream.addr.port(),
            ),
        )
        .expect("live egress reload should update the running proxy");
    fs::write(volume.join("phase2-go"), "go\n").expect("phase-two trigger should write");

    let phase2 = wait_for_result(
        &volume.join("phase2"),
        Duration::from_secs(45),
        "phase-two proxy conformance result",
    );
    assert_result_line(&phase2, "old_endpoint_after_reload=denied");
    assert_result_line(&phase2, "new_endpoint_after_reload=allowed");
    assert_result_line(&phase2, "dns_internal_denied=denied");

    block_on(backend.stop(&handle.id))
        .expect("container should stop and release network artifacts");
    block_on(backend.remove_tenant_artifacts(tenant_id))
        .expect("tenant artifacts should clean up after proof");
    cleanup.disarm();
    drop(temp_dir);
}

fn smoke_config(workdir: &Path, first_port: u16) -> ContainerSandboxBackendConfig {
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

fn test_workdir(name: &str) -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().expect("temporary workdir should be created");
    let workdir = env::var_os("NIMBUS_CONTAINER_EGRESS_WORKDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| temp_dir.path().to_path_buf())
        .join(name);
    fs::create_dir_all(&workdir).expect("egress smoke workdir should exist");
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

fn wait_for_result(path: &Path, timeout: Duration, label: &str) -> String {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(contents) = fs::read_to_string(path) {
            return contents;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("timed out waiting for {label} at {}", path.display());
}

fn assert_result_line(results: &str, expected: &str) {
    assert!(
        results.lines().any(|line| line == expected),
        "expected result line {expected:?}, got:\n{results}"
    );
}

fn proxy_matrix_command(phase_one_port: u16, phase_two_port: u16, direct_target: &str) -> String {
    format!(
        r#"PHASE1_TMP=/nimbus-egress/phase1.tmp
PHASE2_TMP=/nimbus-egress/phase2.tmp
: > "$PHASE1_TMP"
if wget -T 5 -q -O /tmp/allowed_v1 "http://127.0.0.1:{phase_one_port}/allowed" && grep -q allowed-v1 /tmp/allowed_v1; then echo allowed_v1=allowed >> "$PHASE1_TMP"; else echo allowed_v1=denied >> "$PHASE1_TMP"; fi
if wget -T 5 -q -O /tmp/l7_path "http://127.0.0.1:{phase_one_port}/blocked"; then echo l7_path_denied=allowed >> "$PHASE1_TMP"; else echo l7_path_denied=denied >> "$PHASE1_TMP"; fi
if wget -T 5 -q -O /tmp/default_loopback "http://127.0.0.1:{phase_two_port}/allowed"; then echo loopback_default_denied=allowed >> "$PHASE1_TMP"; else echo loopback_default_denied=denied >> "$PHASE1_TMP"; fi
if (unset HTTP_PROXY http_proxy HTTPS_PROXY https_proxy ALL_PROXY all_proxy NO_PROXY no_proxy; wget -T 4 -q -O /tmp/direct_bypass {direct_target:?}); then echo direct_bypass=allowed >> "$PHASE1_TMP"; else echo direct_bypass=denied >> "$PHASE1_TMP"; fi
mv "$PHASE1_TMP" {PHASE1_PATH_IN_GUEST}
while [ ! -f {PHASE2_TRIGGER_IN_GUEST} ]; do sleep 1; done
: > "$PHASE2_TMP"
if wget -T 5 -q -O /tmp/old_after_reload "http://127.0.0.1:{phase_one_port}/allowed"; then echo old_endpoint_after_reload=allowed >> "$PHASE2_TMP"; else echo old_endpoint_after_reload=denied >> "$PHASE2_TMP"; fi
if wget -T 5 -q -O /tmp/allowed_v2 "http://127.0.0.1:{phase_two_port}/allowed" && grep -q allowed-v2 /tmp/allowed_v2; then echo new_endpoint_after_reload=allowed >> "$PHASE2_TMP"; else echo new_endpoint_after_reload=denied >> "$PHASE2_TMP"; fi
if wget -T 5 -q -O /tmp/dns_internal "http://ip6-localhost:{phase_one_port}/private"; then echo dns_internal_denied=allowed >> "$PHASE2_TMP"; else echo dns_internal_denied=denied >> "$PHASE2_TMP"; fi
mv "$PHASE2_TMP" {PHASE2_PATH_IN_GUEST}
sleep 120"#
    )
}

fn phase_one_policy(port: u16) -> EgressPolicy {
    EgressPolicy::new([EgressRule::new(
        "phase-one-allowed",
        EgressProtocol::Http,
        "127.0.0.1",
        port,
    )
    .with_methods(["GET"])
    .with_path_prefixes(["/allowed"])
    .allow_internal_ips(true)])
}

fn phase_two_policy(allowed_port: u16, internal_dns_port: u16) -> EgressPolicy {
    EgressPolicy::new([
        EgressRule::new(
            "phase-two-allowed",
            EgressProtocol::Http,
            "127.0.0.1",
            allowed_port,
        )
        .with_methods(["GET"])
        .with_path_prefixes(["/allowed"])
        .allow_internal_ips(true),
        EgressRule::new(
            "dns-internal-denied",
            EgressProtocol::Http,
            "ip6-localhost",
            internal_dns_port,
        )
        .with_methods(["GET"])
        .with_path_prefixes(["/private"]),
    ])
}

fn assert_root() {
    // SAFETY: geteuid has no preconditions and does not dereference pointers.
    let effective_uid = unsafe { libc::geteuid() };
    assert_eq!(
        effective_uid, 0,
        "container egress smoke test must run as root so it can create network namespaces"
    );
}

struct CleanupGuard {
    backend: ContainerSandboxBackend,
    sandbox_id: SandboxId,
    tenant_id: Option<TenantId>,
}

impl CleanupGuard {
    fn new(backend: ContainerSandboxBackend, sandbox_id: SandboxId, tenant_id: TenantId) -> Self {
        Self {
            backend,
            sandbox_id,
            tenant_id: Some(tenant_id),
        }
    }

    fn disarm(mut self) {
        self.tenant_id = None;
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let Some(tenant_id) = self.tenant_id.take() else {
            return;
        };
        let _ = block_on(self.backend.stop(&self.sandbox_id));
        let _ = block_on(self.backend.remove_tenant_artifacts(tenant_id));
    }
}

struct TestHttpServer {
    addr: SocketAddr,
}

impl TestHttpServer {
    fn start(body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
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
