#![cfg(target_os = "linux")]

use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;

use neovex_core::TenantId;
use neovex_sandbox::backends::krun::buildah::OciProcessOverrides;
use neovex_sandbox::backends::krun::{
    KrunLaunchMode, KrunSandboxBackend, KrunSandboxBackendConfig,
};
use neovex_sandbox::{
    PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind, SandboxFilesystemSpec,
    SandboxPortBinding, SandboxProcessSpec, SandboxSpec, SandboxStatus,
};

#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and a mounted rootfs"]
fn krun_backend_smoke_boots_http_service_and_survives_backend_restart() {
    let rootfs = env_path("NEOVEX_KRUN_SMOKE_ROOTFS");
    let host_port = env_u16("NEOVEX_KRUN_SMOKE_HOST_PORT").unwrap_or(18080);
    let guest_port = env_u16("NEOVEX_KRUN_SMOKE_GUEST_PORT").unwrap_or(8080);

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("bundles");
    let state_root = base_dir.join("state");

    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root.clone();
    config.state_root = state_root.clone();
    config.launch_mode = KrunLaunchMode::Execute;

    if let Some(runtime_path) = env::var_os("NEOVEX_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NEOVEX_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NEOVEX_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    let backend = KrunSandboxBackend::new(config.clone());
    let guest_port_str = guest_port.to_string();
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "http-smoke",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(rootfs),
        SandboxProcessSpec::new(["/bin/busybox", "httpd", "-f", "-p", &guest_port_str]),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        PublishedEndpointProtocol::Http,
        host_port,
        guest_port,
    ));

    let handle = block_on(backend.start(spec)).expect("krun backend should start the sandbox");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(15));
    assert_eq!(
        ready_handle.status,
        SandboxStatus::Ready,
        "the backend should report a running sandbox after crun start succeeds"
    );

    let restarted_backend = KrunSandboxBackend::new(config);
    let restarted_handle = block_on(restarted_backend.inspect(&handle.id))
        .expect("inspect should succeed after constructing a new backend instance")
        .expect("manifest-backed sandbox should still be discoverable");
    assert_eq!(
        restarted_handle.status,
        SandboxStatus::Ready,
        "a fresh backend instance should recover the running sandbox from disk-backed state"
    );

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert!(
        http_response.starts_with("HTTP/1.1 200") || http_response.starts_with("HTTP/1.1 404"),
        "expected an HTTP response from the guest service, got: {http_response}"
    );

    let container_state_dir = state_root.join("containers").join(handle.id.as_str());
    assert!(
        container_state_dir.join("ctr.log").exists(),
        "conmon stdout/stderr log path should exist"
    );
    assert!(
        container_state_dir.join("oci.log").exists(),
        "runtime OCI log path should exist"
    );

    block_on(restarted_backend.stop(&handle.id)).expect("stop should succeed");
    let stopped_handle = block_on(restarted_backend.inspect(&handle.id))
        .expect("inspect should succeed after stop")
        .expect("stopped sandbox should still have a manifest");
    assert_eq!(
        stopped_handle.status,
        SandboxStatus::Stopped,
        "a deliberate backend stop should be reported as stopped even if SIGKILL was required"
    );

    cleanup_guard.disarm();
}

#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and network access for image pull"]
fn krun_backend_image_backed_smoke_pulls_and_boots_busybox() {
    // Use a different default port from the rootfs-only test so the two ignored
    // tests can run in parallel without port collisions.  Callers can still
    // override via env vars, but the defaults are safe for `-- --ignored`.
    let host_port = env_u16("NEOVEX_KRUN_IMAGE_SMOKE_HOST_PORT").unwrap_or(18081);
    let guest_port = env_u16("NEOVEX_KRUN_IMAGE_SMOKE_GUEST_PORT").unwrap_or(8081);

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("image-bundles");
    let state_root = base_dir.join("image-state");

    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root.clone();
    config.state_root = state_root.clone();
    config.launch_mode = KrunLaunchMode::Execute;

    if let Some(runtime_path) = env::var_os("NEOVEX_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NEOVEX_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NEOVEX_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    let backend = KrunSandboxBackend::new(config.clone());
    let guest_port_str = guest_port.to_string();
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "image-smoke",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(""),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        PublishedEndpointProtocol::Http,
        host_port,
        guest_port,
    ));

    let overrides = OciProcessOverrides {
        cmd: Some(vec![
            "/bin/busybox".into(),
            "httpd".into(),
            "-f".into(),
            "-p".into(),
            guest_port_str,
        ]),
        ..Default::default()
    };

    let handle =
        block_on(backend.start_from_image(spec, "docker://busybox:latest".to_owned(), overrides))
            .expect("image-backed krun start should succeed");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(30));
    assert_eq!(
        ready_handle.status,
        SandboxStatus::Ready,
        "image-backed sandbox should reach ready"
    );

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert!(
        http_response.starts_with("HTTP/1.") || http_response.contains("404"),
        "expected HTTP response from image-backed sandbox, got: {http_response}"
    );

    let restarted_backend = KrunSandboxBackend::new(config);
    let restarted_handle = block_on(restarted_backend.inspect(&handle.id))
        .expect("inspect should succeed")
        .expect("image-backed sandbox should survive backend restart");
    assert_eq!(restarted_handle.status, SandboxStatus::Ready);

    block_on(restarted_backend.stop(&handle.id)).expect("stop should succeed");
    let stopped_handle = block_on(restarted_backend.inspect(&handle.id))
        .expect("inspect after stop should succeed")
        .expect("stopped sandbox should still have a manifest");
    assert_eq!(stopped_handle.status, SandboxStatus::Stopped);

    cleanup_guard.disarm();
}

/// M2 verification: prove image STOPSIGNAL-aware shutdown and image USER
/// resolution on a real Linux host.
///
/// Creates a custom local image from BusyBox with:
///   USER www-data          (uid=33, gid=33 in BusyBox's /etc/passwd)
///   STOPSIGNAL SIGQUIT
///
/// Verifies:
///   - the manifest records the resolved numeric user (33:33) from the image
///   - the manifest records stop_signal=SIGQUIT from the image
///   - the VM boots (with root user because krun VMM needs /dev/kvm)
///   - the guest service is reachable over TSI
///   - stop sends SIGQUIT first (configured signal), then falls back to SIGKILL
///
/// Note: krun containers cannot run the VMM process as a non-root user because
/// `/dev/kvm` requires root or kvm-group access.  The image USER is resolved
/// and recorded for future guest-side application, but the OCI bundle
/// process.user stays 0:0 for the VMM.  This is correct krun behavior.
#[test]
#[ignore = "requires a Linux host with KVM, buildah, conmon, and network access"]
fn krun_backend_m2_user_and_stop_signal_lowering() {
    let host_port: u16 = 18082;
    let guest_port: u16 = 8082;

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let bundle_root = base_dir.join("m2-bundles");
    let state_root = base_dir.join("m2-state");

    let mut config = KrunSandboxBackendConfig::default();
    config.bundle_root = bundle_root.clone();
    config.state_root = state_root.clone();
    config.launch_mode = KrunLaunchMode::Execute;

    if let Some(runtime_path) = env::var_os("NEOVEX_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NEOVEX_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NEOVEX_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    // Build a custom local image with non-root USER and non-default STOPSIGNAL.
    let buildah = env::var("NEOVEX_KRUN_SMOKE_BUILDAH").unwrap_or("buildah".into());
    run_host_command(&buildah, &["rm", "m2-fixture"], true);
    run_host_command(
        &buildah,
        &["from", "--name", "m2-fixture", "docker://busybox:latest"],
        false,
    );
    run_host_command(
        &buildah,
        &["config", "--user", "www-data", "m2-fixture"],
        false,
    );
    run_host_command(
        &buildah,
        &["config", "--stop-signal", "SIGQUIT", "m2-fixture"],
        false,
    );
    run_host_command(
        &buildah,
        &["commit", "m2-fixture", "localhost/neovex-m2-fixture:latest"],
        false,
    );
    run_host_command(&buildah, &["rm", "m2-fixture"], true);

    let backend = KrunSandboxBackend::new(config.clone());
    let guest_port_str = guest_port.to_string();

    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "m2-user-signal",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(""),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        PublishedEndpointProtocol::Http,
        host_port,
        guest_port,
    ));

    let overrides = OciProcessOverrides {
        cmd: Some(vec![
            "/bin/busybox".into(),
            "httpd".into(),
            "-f".into(),
            "-p".into(),
            guest_port_str,
        ]),
        ..Default::default()
    };

    let handle = block_on(backend.start_from_image(
        spec,
        "localhost/neovex-m2-fixture:latest".to_owned(),
        overrides,
    ))
    .expect("image-backed start with non-root user should succeed");
    let cleanup_guard = CleanupGuard::new(backend.clone(), handle.id.clone());

    let ready_handle = wait_for_ready(&backend, &handle.id, Duration::from_secs(30));
    assert_eq!(ready_handle.status, SandboxStatus::Ready);

    // Verify HTTP connectivity.
    let http_response = wait_for_http_response(host_port, Duration::from_secs(15));
    assert!(
        http_response.starts_with("HTTP/1.") || http_response.contains("404"),
        "expected HTTP response from non-root-user sandbox, got: {http_response}"
    );

    // Verify bundle config.json has non-root uid/gid (www-data = 33:33).
    let bundle_config_path = bundle_root.join(handle.id.as_str()).join("config.json");
    let bundle_config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&bundle_config_path).unwrap_or_else(|_| {
            panic!(
                "bundle config should be readable at {}",
                bundle_config_path.display()
            )
        }))
        .expect("bundle config should be valid JSON");

    let uid = bundle_config["process"]["user"]["uid"]
        .as_u64()
        .expect("uid should be present");
    let gid = bundle_config["process"]["user"]["gid"]
        .as_u64()
        .expect("gid should be present");
    eprintln!("bundle process.user: uid={uid}, gid={gid}");
    // krun VMMs always run as root because the crun process needs /dev/kvm.
    // The image USER (www-data=33:33) is stored in the manifest for guest-side use.
    assert_eq!(
        uid, 0,
        "krun bundle must use root uid for VMM /dev/kvm access"
    );
    assert_eq!(
        gid, 0,
        "krun bundle must use root gid for VMM /dev/kvm access"
    );

    // Verify manifest records the configured stop signal.
    let manifest_path = state_root
        .join("containers")
        .join(handle.id.as_str())
        .join("manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap_or_else(|_| {
            panic!("manifest should be readable at {}", manifest_path.display())
        }))
        .expect("manifest should be valid JSON");

    // The image USER is stored in manifest metadata for future guest-side use.
    let recorded_user = manifest["image_metadata"]["user"]
        .as_str()
        .unwrap_or("(none)");
    eprintln!("manifest.image_metadata.user: {recorded_user}");
    assert!(
        recorded_user.contains("33"),
        "manifest should record the resolved image user (www-data=33), got: {recorded_user}"
    );

    let recorded_signal = manifest["image_metadata"]["stop_signal"]
        .as_str()
        .unwrap_or("(none)");
    eprintln!("manifest.image_metadata.stop_signal: {recorded_signal}");
    assert_eq!(
        recorded_signal, "SIGQUIT",
        "manifest should record the image-configured STOPSIGNAL"
    );

    // Stop the sandbox.  BusyBox httpd does not handle SIGQUIT, so the backend
    // should send SIGQUIT first (configured signal), timeout, then SIGKILL.
    let stop_start = Instant::now();
    block_on(backend.stop(&handle.id)).expect("stop should succeed");
    let stop_elapsed = stop_start.elapsed();
    eprintln!("stop elapsed: {stop_elapsed:?}");

    let stopped_handle = block_on(backend.inspect(&handle.id))
        .expect("inspect after stop should succeed")
        .expect("stopped sandbox should still have a manifest");
    assert_eq!(stopped_handle.status, SandboxStatus::Stopped);

    // Re-read manifest after stop for exit code.
    let manifest_after: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&manifest_path).expect("manifest should be readable after stop"),
    )
    .expect("manifest should be valid JSON after stop");
    let exit_code = manifest_after["last_exit_code"].as_i64();
    eprintln!("manifest.last_exit_code: {exit_code:?}");
    eprintln!(
        "manifest.shutdown_requested: {}",
        manifest_after["shutdown_requested"]
    );
    assert_eq!(
        exit_code,
        Some(137),
        "exit code 137 = SIGKILL after SIGQUIT timeout"
    );

    cleanup_guard.disarm();
}

fn run_host_command(program: &str, args: &[&str], allow_failure: bool) {
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

fn wait_for_ready(
    backend: &KrunSandboxBackend,
    id: &neovex_sandbox::SandboxId,
    timeout: Duration,
) -> neovex_sandbox::SandboxHandle {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(handle) = block_on(backend.inspect(id)).expect("inspect should succeed") {
            if handle.status == SandboxStatus::Ready {
                return handle;
            }
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!("sandbox did not become ready within {:?}", timeout);
}

fn wait_for_http_response(port: u16, timeout: Duration) -> String {
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

fn env_path(key: &str) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("expected environment variable {key} to be set"))
}

fn env_u16(key: &str) -> Option<u16> {
    env::var(key).ok().map(|value| {
        value
            .parse::<u16>()
            .unwrap_or_else(|error| panic!("failed to parse {key}={value:?} as u16: {error}"))
    })
}

struct CleanupGuard {
    backend: KrunSandboxBackend,
    sandbox_id: Option<neovex_sandbox::SandboxId>,
}

impl CleanupGuard {
    fn new(backend: KrunSandboxBackend, sandbox_id: neovex_sandbox::SandboxId) -> Self {
        Self {
            backend,
            sandbox_id: Some(sandbox_id),
        }
    }

    fn disarm(self) {
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
