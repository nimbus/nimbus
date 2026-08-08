use super::*;

pub(super) use super::provision_support::provision_krun;

pub(super) fn smoke_backend_config(
    bundle_root: PathBuf,
    state_root: PathBuf,
) -> KrunSandboxBackendConfig {
    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root;
    config.workload_state_root = state_root.clone();
    config.network_state_root = state_root;
    config.start_mode = KrunStartMode::Execute;

    if let Some(runtime_path) = env::var_os("NIMBUS_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NIMBUS_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NIMBUS_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    config
}

pub(super) fn sandbox_tenant() -> TenantId {
    TenantId::new("tenant").expect("tenant id should be valid")
}

pub(super) fn http_binding(host_port: u16, guest_port: u16) -> SandboxPortBinding {
    SandboxPortBinding::new("http", EndpointProtocol::Http, host_port, guest_port)
}

pub(super) fn rootfs_spec(name: &str, rootfs: impl Into<PathBuf>) -> SandboxSpec {
    SandboxSpec::new(
        sandbox_tenant(),
        SandboxOwnerSpec::standalone_named(name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs(rootfs),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
}

pub(super) fn image_spec(name: &str, image_reference: impl Into<String>) -> SandboxSpec {
    SandboxSpec::new(
        sandbox_tenant(),
        SandboxOwnerSpec::standalone_named(name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image_reference),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
}

pub(super) fn busybox_http_process(guest_port: u16) -> SandboxProcessSpec {
    SandboxProcessSpec::new(Vec::<String>::new()).with_command([
        "/bin/busybox".into(),
        "httpd".into(),
        "-f".into(),
        "-p".into(),
        guest_port.to_string(),
    ])
}

pub(super) fn buildah_program() -> String {
    env::var("NIMBUS_KRUN_SMOKE_BUILDAH").unwrap_or_else(|_| "buildah".into())
}

pub(super) fn assert_httpish_response(response: &str, context: &str) {
    assert!(
        response.starts_with("HTTP/1.") || response.contains("404"),
        "{context}, got: {response}"
    );
}

pub(super) fn assert_host_port_not_bound_to_non_loopback(port: u16) {
    let Some(host_address) = non_loopback_host_address() else {
        eprintln!("skipping non-loopback bind probe because no host address was discovered");
        return;
    };
    let probe_address = std::net::SocketAddr::new(host_address, port);

    match TcpStream::connect_timeout(&probe_address, Duration::from_secs(2)) {
        Ok(_) => panic!(
            "krun host port {port} accepted a connection on non-loopback address {host_address}"
        ),
        Err(error) => eprintln!("non-loopback bind probe {probe_address}: {error}"),
    }
}

fn non_loopback_host_address() -> Option<std::net::IpAddr> {
    if let Some(configured_address) = env::var("NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        let parsed = configured_address.parse::<std::net::IpAddr>().unwrap_or_else(|error| {
            panic!(
                "failed to parse NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST={configured_address:?}: {error}"
            )
        });
        assert!(
            !parsed.is_loopback() && !parsed.is_unspecified(),
            "NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST must be a non-loopback host address"
        );
        return Some(parsed);
    }

    let output = std::process::Command::new("hostname")
        .arg("-I")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|candidate| candidate.parse::<std::net::IpAddr>().ok())
        .find(|address| !address.is_loopback() && !address.is_unspecified())
}

pub(super) fn run_host_command(program: &str, args: &[&str], allow_failure: bool) {
    let status = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap_or_else(|e| panic!("failed to run {program} {}: {e}", args.join(" ")));
    if !allow_failure && !status.success() {
        panic!("{program} {} failed with {status}", args.join(" "));
    }
}

pub(super) fn run_host_command_capture_stdout(program: &str, args: &[&str]) -> String {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {program} {}: {e}", args.join(" ")));
    if !output.status.success() {
        panic!("{program} {} failed with {}", args.join(" "), output.status);
    }
    String::from_utf8(output.stdout)
        .unwrap_or_else(|e| panic!("stdout from {program} was not utf-8: {e}"))
}

pub(super) fn read_manifest_mount_session_name(
    state_root: &std::path::Path,
    sandbox_id: &nimbus_sandbox::SandboxId,
) -> String {
    let tenant_id = sandbox_tenant();
    let manifest_path = manifest_path(state_root, &tenant_id, sandbox_id);
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap_or_else(|_| {
            panic!("manifest should be readable at {}", manifest_path.display())
        }))
        .expect("manifest should be valid JSON");
    manifest["launch_artifact"]["MountedRootfs"]["session_name"]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "manifest {} should record a mounted rootfs session name",
                manifest_path.display()
            )
        })
        .to_owned()
}

pub(super) fn bundle_config_path(
    bundle_root: &std::path::Path,
    tenant_id: &TenantId,
    sandbox_id: &nimbus_sandbox::SandboxId,
) -> PathBuf {
    bundle_root
        .join("tenants")
        .join(tenant_id.as_str())
        .join("sandboxes")
        .join(sandbox_id.as_str())
        .join("bundle")
        .join("config.json")
}

pub(super) fn container_state_dir(
    state_root: &std::path::Path,
    tenant_id: &TenantId,
    sandbox_id: &nimbus_sandbox::SandboxId,
) -> PathBuf {
    state_root
        .join("tenants")
        .join(tenant_id.as_str())
        .join("sandboxes")
        .join(sandbox_id.as_str())
        .join("state")
        .join("containers")
        .join(sandbox_id.as_str())
}

pub(super) fn manifest_path(
    state_root: &std::path::Path,
    tenant_id: &TenantId,
    sandbox_id: &nimbus_sandbox::SandboxId,
) -> PathBuf {
    container_state_dir(state_root, tenant_id, sandbox_id).join("manifest.json")
}

pub(super) fn read_buildah_rootfs_file(
    buildah_program: &str,
    container_name: &str,
    relative_path: &str,
) -> String {
    let script = r#"rootfs="$("$1" mount "$2")"
test -n "$rootfs"
cat "$rootfs/$3""#;
    run_host_command_capture_stdout(
        buildah_program,
        &[
            "unshare",
            "--",
            "sh",
            "-c",
            script,
            "nimbus-buildah-unshare",
            buildah_program,
            container_name,
            relative_path,
        ],
    )
}

pub(super) fn wait_for_ready(
    backend: &KrunSandboxBackend,
    id: &nimbus_sandbox::SandboxId,
    timeout: Duration,
) -> nimbus_sandbox::SandboxHandle {
    wait_for_status(backend, id, SandboxStatus::Ready, timeout)
}

pub(super) fn wait_for_status(
    backend: &KrunSandboxBackend,
    id: &nimbus_sandbox::SandboxId,
    expected: SandboxStatus,
    timeout: Duration,
) -> nimbus_sandbox::SandboxHandle {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(inspection) = block_on(backend.inspect(id))
            .expect("inspect should succeed")
            .filter(|inspection| inspection.handle.status == expected)
        {
            return inspection.handle;
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!("sandbox did not reach {expected:?} within {:?}", timeout);
}

pub(super) fn wait_for_http_response(port: u16, timeout: Duration) -> String {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("read timeout should be settable");
                stream
                    .write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
                    .expect("HTTP probe should be writable");

                let mut response = vec![0u8; 4096];
                match stream.read(&mut response) {
                    Ok(n) if n > 0 => {
                        let text = String::from_utf8_lossy(&response[..n]).to_string();
                        return text;
                    }
                    Ok(_) => eprintln!("HTTP probe connected but got empty response"),
                    Err(error) => eprintln!("HTTP probe read error: {error}"),
                }
            }
            Err(error) => {
                eprintln!("HTTP probe connect error on port {port}: {error}");
            }
        }
        thread::sleep(Duration::from_millis(500));
    }

    panic!(
        "guest service did not answer HTTP on port {port} within {:?}",
        timeout
    );
}

pub(super) fn wait_for_http_unreachable(port: u16, timeout: Duration) {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
            Ok(mut stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                if stream
                    .write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
                    .is_err()
                {
                    return;
                }

                let mut response = [0u8; 256];
                match stream.read(&mut response) {
                    Ok(0) => return,
                    Err(_) => return,
                    Ok(_) => {}
                }
            }
            Err(_) => return,
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!(
        "guest service on port {port} remained reachable for {:?}",
        timeout
    );
}

pub(super) fn env_path(key: &str) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("expected environment variable {key} to be set"))
}

pub(super) fn env_u16(key: &str) -> Option<u16> {
    env::var(key).ok().map(|value| {
        value
            .parse::<u16>()
            .unwrap_or_else(|error| panic!("failed to parse {key}={value:?} as u16: {error}"))
    })
}

pub(super) struct CleanupGuard {
    backend: KrunSandboxBackend,
    sandbox_id: Option<nimbus_sandbox::SandboxId>,
}

impl CleanupGuard {
    pub(super) fn new(backend: KrunSandboxBackend, sandbox_id: nimbus_sandbox::SandboxId) -> Self {
        Self {
            backend,
            sandbox_id: Some(sandbox_id),
        }
    }

    pub(super) fn disarm(self) {
        std::mem::forget(self);
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if let Some(sandbox_id) = self.sandbox_id.take() {
            let _ = block_on(self.backend.stop(&sandbox_id));
        }
    }
}
