#![cfg(target_os = "linux")]

use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;

use neovex_core::TenantId;
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
