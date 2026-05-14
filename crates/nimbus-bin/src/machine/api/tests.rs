use super::binaries::{STANDARD_CONTAINER_BINARY_REQUIREMENTS, apply_resolved_runtime_paths};
use super::capabilities::machine_api_capability_response;
use super::listener::{
    inherited_systemd_listener, remove_env_var, set_env_var, tokio_listener_from_inherited_fd,
};
use super::state::machine_container_state_root;
use super::*;

use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus::{
    PublishedEndpoint, PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind,
    SandboxBuildLaunchSpec, SandboxError, SandboxHandle, SandboxId, SandboxImageLaunchSpec,
    SandboxPortBinding, SandboxSpec, SandboxStatus, TenantId,
};
use nimbus_sandbox::SandboxFuture;
use serde_json::json;
use tempfile::{Builder, TempDir};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_serves_health_and_capabilities_over_unix_socket() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_backend: None,
        machine_port_forwarder: None,
    };
    for requirement in STANDARD_CONTAINER_BINARY_REQUIREMENTS {
        write_fake_binary(&temp_dir, requirement.name);
    }
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let health = wait_for_http_response_contains(&socket_path, "/healthz", "\"status\":\"ok\"");
    assert!(health.contains("200 OK"), "{health}");
    assert!(health.contains("\"status\":\"ok\""), "{health}");
    assert!(
        health.contains("\"role\":\"guest-machine-api\""),
        "{health}"
    );

    let capabilities = unix_http_get(&socket_path, "/v1/machine-api/capabilities");
    assert!(capabilities.contains("200 OK"), "{capabilities}");
    assert!(
        capabilities.contains("\"service_execution_ready\":false"),
        "{capabilities}"
    );
    assert!(
        capabilities.contains("\"service_execution_mode\":\"standard_containers\""),
        "{capabilities}"
    );
    assert!(
        capabilities.contains("\"supported_service_backends\":[\"container\"]"),
        "{capabilities}"
    );
    assert!(
        capabilities.contains("\"service_execution_blockers\":["),
        "{capabilities}"
    );
    assert!(
        capabilities
            .contains("\"guest machine API does not yet expose service lifecycle operations\""),
        "{capabilities}"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

#[test]
fn capability_response_reports_binary_statuses_and_explicit_blockers() {
    let temp_dir = short_socket_tempdir();
    write_fake_binary(&temp_dir, "conmon");
    write_fake_binary(&temp_dir, "crun");

    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_backend: None,
        machine_port_forwarder: None,
    };
    let capabilities = machine_api_capability_response(&state);

    assert_eq!(
        capabilities.service_execution_mode,
        MachineApiServiceExecutionMode::StandardContainers
    );
    assert_eq!(
        capabilities.supported_service_backends,
        vec![SandboxBackendKind::Container]
    );
    assert_eq!(
        capabilities.supported_operations,
        vec!["healthz".to_owned(), "capabilities".to_owned()]
    );
    assert!(!capabilities.service_execution_ready);
    assert!(
        capabilities
            .service_execution_blockers
            .iter()
            .any(|blocker| blocker == MACHINE_API_OPERATION_BLOCKER)
    );
    assert!(capabilities.binary_statuses.iter().any(|binary| {
        binary.name == "netavark"
            && !binary.present
            && binary.required_for_operations
                == vec![
                    MACHINE_API_IMAGE_START_OPERATION.to_owned(),
                    MACHINE_API_BUILD_START_OPERATION.to_owned(),
                ]
    }));
    assert!(
        capabilities
            .service_execution_blockers
            .iter()
            .any(|blocker| blocker
                == "missing guest binary required for service-sandboxes.image-start: netavark")
    );
    assert!(capabilities.operation_statuses.iter().any(|status| {
        status.name == MACHINE_API_BUILD_START_OPERATION
            && !status.available
            && status
                .blockers
                .iter()
                .any(|blocker| blocker == MACHINE_API_OPERATION_BLOCKER)
    }));
}

#[test]
fn capability_response_reports_machine_port_forwarder_blocker_when_unreachable() {
    let temp_dir = short_socket_tempdir();
    for requirement in STANDARD_CONTAINER_BINARY_REQUIREMENTS {
        write_fake_binary(&temp_dir, requirement.name);
    }

    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_backend: Some(Arc::new(ContainerSandboxBackend::new(
            ContainerSandboxBackendConfig::plan_only(
                temp_dir.path().join("bundles"),
                temp_dir.path().join("state"),
            ),
        ))),
        machine_port_forwarder: Some(OciMachinePortForwarderConfig {
            host: "127.0.0.1".to_owned(),
            port: 9,
            path_prefix: "/services/forwarder".to_owned(),
        }),
    };

    let capabilities = machine_api_capability_response(&state);
    assert!(!capabilities.service_execution_ready);
    assert_eq!(
        capabilities.supported_operations,
        vec![
            "healthz".to_owned(),
            "capabilities".to_owned(),
            MACHINE_API_BOOTC_STATUS_OPERATION.to_owned(),
            MACHINE_API_BOOTC_SWITCH_OPERATION.to_owned(),
            MACHINE_API_BOOTC_UPGRADE_OPERATION.to_owned(),
            MACHINE_API_BOOTC_ROLLBACK_OPERATION.to_owned(),
            "service-sandboxes.list".to_owned(),
            "service-sandboxes.inspect".to_owned(),
            "service-sandboxes.inspect-current".to_owned(),
            "service-sandboxes.logs".to_owned(),
            "service-sandboxes.ps".to_owned(),
        ]
    );
    assert!(
        capabilities
            .service_execution_blockers
            .iter()
            .any(|blocker| blocker
                .contains("guest machine port forwarder is not reachable at 127.0.0.1:9")),
        "{:?}",
        capabilities.service_execution_blockers
    );
}

#[test]
fn capability_response_resolves_helper_binaries_from_podman_dirs() {
    let temp_dir = short_socket_tempdir();
    let helper_dir = temp_dir.path().join("podman-helpers");
    fs::create_dir_all(&helper_dir).expect("helper dir should create");
    write_fake_binary(&temp_dir, "conmon");
    write_fake_binary(&temp_dir, "crun");
    write_fake_binary_at(&helper_dir, "netavark");
    write_fake_binary_at(&helper_dir, "aardvark-dns");

    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: vec![helper_dir.clone()],
        service_backend: None,
        machine_port_forwarder: None,
    };

    let capabilities = machine_api_capability_response(&state);
    let netavark_path = helper_dir.join("netavark").display().to_string();
    let aardvark_path = helper_dir.join("aardvark-dns").display().to_string();
    assert!(capabilities.binary_statuses.iter().any(|binary| {
        binary.name == "netavark"
            && binary.present
            && binary.resolved_path.as_deref() == Some(netavark_path.as_str())
    }));
    assert!(capabilities.binary_statuses.iter().any(|binary| {
        binary.name == "aardvark-dns"
            && binary.present
            && binary.resolved_path.as_deref() == Some(aardvark_path.as_str())
    }));
    assert!(
        !capabilities
            .service_execution_blockers
            .iter()
            .any(|blocker| blocker.contains("netavark") || blocker.contains("aardvark-dns"))
    );
}

#[test]
fn capability_response_keeps_build_start_available_without_buildah_or_fuse_overlayfs() {
    let temp_dir = short_socket_tempdir();
    let helper_dir = temp_dir.path().join("podman-helpers");
    fs::create_dir_all(&helper_dir).expect("helper dir should create");
    write_fake_binary(&temp_dir, "conmon");
    write_fake_binary(&temp_dir, "crun");
    write_fake_binary_at(&helper_dir, "netavark");
    write_fake_binary_at(&helper_dir, "aardvark-dns");

    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: vec![helper_dir],
        service_backend: Some(Arc::new(ContainerSandboxBackend::new(
            ContainerSandboxBackendConfig::plan_only(
                temp_dir.path().join("bundles"),
                temp_dir.path().join("state"),
            ),
        ))),
        machine_port_forwarder: None,
    };

    let capabilities = machine_api_capability_response(&state);

    assert!(capabilities.service_execution_ready);
    assert!(
        capabilities
            .supported_operations
            .iter()
            .any(|operation| operation == MACHINE_API_IMAGE_START_OPERATION)
    );
    assert!(
        capabilities
            .supported_operations
            .iter()
            .any(|operation| operation == MACHINE_API_BUILD_START_OPERATION)
    );
    assert!(
        capabilities
            .binary_statuses
            .iter()
            .all(|binary| binary.name != "buildah" && binary.name != "fuse-overlayfs")
    );
    assert!(capabilities.operation_statuses.iter().any(|status| {
        status.name == MACHINE_API_BUILD_START_OPERATION
            && status.available
            && status.blockers.is_empty()
    }));
}

#[test]
fn apply_resolved_runtime_paths_updates_backend_config_from_helper_dirs() {
    let temp_dir = short_socket_tempdir();
    let helper_dir = temp_dir.path().join("podman-helpers");
    fs::create_dir_all(&helper_dir).expect("helper dir should create");
    write_fake_binary(&temp_dir, "buildah");
    write_fake_binary(&temp_dir, "conmon");
    write_fake_binary(&temp_dir, "crun");
    write_fake_binary_at(&helper_dir, "netavark");
    write_fake_binary_at(&helper_dir, "aardvark-dns");

    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path().join("root"));
    let runtime_path = fake_runtime_path(&temp_dir);
    apply_resolved_runtime_paths(
        &mut config,
        Some(runtime_path.as_os_str()),
        std::slice::from_ref(&helper_dir),
    );

    assert_eq!(config.buildah_path, temp_dir.path().join("buildah"));
    assert_eq!(config.conmon_path, temp_dir.path().join("conmon"));
    assert_eq!(config.runtime_path, temp_dir.path().join("crun"));
    assert_eq!(config.netavark_path, helper_dir.join("netavark"));
    assert_eq!(config.aardvark_dns_path, helper_dir.join("aardvark-dns"));
}

#[test]
fn socket_activation_listener_requires_matching_systemd_env() {
    let _guard = MachineApiEnvGuard::capture();
    set_env_var("LISTEN_PID", "999999");
    set_env_var("LISTEN_FDS", "1");

    let error = inherited_systemd_listener().expect_err("pid mismatch should fail");
    assert!(
        error
            .to_string()
            .contains("machine API socket activation expected LISTEN_PID=")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn socket_activation_listener_accepts_one_inherited_fd() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = StdUnixListener::bind(&socket_path).expect("listener should bind");
    let duplicated_fd = unsafe { libc::dup(listener.as_raw_fd()) };
    assert!(duplicated_fd >= 0, "listener fd should duplicate");

    let tokio_listener =
        tokio_listener_from_inherited_fd(duplicated_fd).expect("fd should convert");
    drop(tokio_listener);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_list_and_current_refresh_persisted_service_state_before_reply() {
    let temp_dir = short_socket_tempdir();
    let control_data_dir = temp_dir.path().join("control");
    let state_root = machine_container_state_root(&control_data_dir);
    let tenant_id = TenantId::new("svc-demo").expect("tenant id should be valid");
    let sandbox_id = SandboxId::new("demo-01aaa");
    let stopped_sandbox_id = SandboxId::new("demo-01old");
    write_container_manifest(
        &state_root,
        sandbox_id.as_str(),
        tenant_id.as_str(),
        "demo",
        SandboxStatus::Starting,
        Vec::new(),
    );
    write_container_manifest(
        &state_root,
        stopped_sandbox_id.as_str(),
        tenant_id.as_str(),
        "demo",
        SandboxStatus::Stopped,
        Vec::new(),
    );

    let backend = RefreshingInspectBackend::new(state_root.clone());
    let inspected_ids = backend.inspected_ids();

    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let state = MachineApiState {
        control_data_dir,
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_backend: Some(Arc::new(backend)),
        machine_port_forwarder: None,
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let list_response = wait_for_http_response_contains(
        &socket_path,
        &format!("/v1/machine-api/service-sandboxes?tenant_id={tenant_id}"),
        "\"status\":\"ready\"",
    );
    assert!(
        list_response.contains("\"published_endpoints\":[{\"name\":\"default\""),
        "{list_response}"
    );

    let current_response = wait_for_http_response_contains(
        &socket_path,
        &format!(
            "/v1/machine-api/service-sandboxes/current?tenant_id={tenant_id}&service_name=demo"
        ),
        "\"status\":\"ready\"",
    );
    assert!(
        current_response.contains("\"published_endpoints\":[{\"name\":\"default\""),
        "{current_response}"
    );
    let inspected_ids = inspected_ids.lock().expect("lock should acquire").clone();
    assert_eq!(
        inspected_ids,
        vec![
            sandbox_id.as_str().to_owned(),
            sandbox_id.as_str().to_owned()
        ]
    );
    assert!(
        !inspected_ids
            .iter()
            .any(|id| id == stopped_sandbox_id.as_str()),
        "{inspected_ids:?}"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

fn unix_http_get(socket_path: &Path, path: &str) -> String {
    let mut stream = UnixStream::connect(socket_path).expect("unix socket should accept");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should set");
    write!(stream, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").expect("request should write");
    let mut response = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => response.extend_from_slice(&chunk[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => panic!("response should read: {error}"),
        }
    }
    String::from_utf8(response).expect("response should be valid utf-8")
}

fn wait_for_http_response_contains(socket_path: &Path, path: &str, needle: &str) -> String {
    let start = std::time::Instant::now();
    loop {
        let response = try_unix_http_get(socket_path, path).unwrap_or_default();
        if response.contains(needle) {
            return response;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed out waiting for machine API response on {}{}; last response: {}",
            socket_path.display(),
            path,
            response
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn try_unix_http_get(socket_path: &Path, path: &str) -> Result<String, std::io::Error> {
    let mut stream = UnixStream::connect(socket_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    write!(stream, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
    let mut response = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => response.extend_from_slice(&chunk[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    String::from_utf8(response)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

fn wait_for_socket_path(path: &Path) {
    let start = std::time::Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed out waiting for socket {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct MachineApiEnvGuard {
    listen_pid: Option<String>,
    listen_fds: Option<String>,
}

impl MachineApiEnvGuard {
    fn capture() -> Self {
        Self {
            listen_pid: std::env::var("LISTEN_PID").ok(),
            listen_fds: std::env::var("LISTEN_FDS").ok(),
        }
    }
}

impl Drop for MachineApiEnvGuard {
    fn drop(&mut self) {
        match &self.listen_pid {
            Some(value) => set_env_var("LISTEN_PID", value),
            None => remove_env_var("LISTEN_PID"),
        }
        match &self.listen_fds {
            Some(value) => set_env_var("LISTEN_FDS", value),
            None => remove_env_var("LISTEN_FDS"),
        }
    }
}

fn short_socket_tempdir() -> TempDir {
    Builder::new()
        .prefix("nimbus-ma-")
        .tempdir_in("/tmp")
        .expect("short temp dir should exist")
}

fn fake_runtime_path(temp_dir: &TempDir) -> OsString {
    temp_dir.path().as_os_str().to_owned()
}

fn write_fake_binary(temp_dir: &TempDir, name: &str) {
    write_fake_binary_at(temp_dir.path(), name);
}

fn write_fake_binary_at(root: &Path, name: &str) {
    let path = root.join(name);
    crate::test_support::write_executable_stub(&path, "#!/bin/sh\nexit 0\n");
}

fn write_container_manifest(
    state_root: &Path,
    sandbox_id: &str,
    tenant_id: &str,
    service_name: &str,
    status: SandboxStatus,
    published_endpoints: Vec<PublishedEndpoint>,
) {
    let container_dir = state_root.join("containers").join(sandbox_id);
    fs::create_dir_all(&container_dir).expect("container manifest directory should exist");

    let handle = SandboxHandle::new(
        SandboxId::new(sandbox_id),
        service_name,
        SandboxBackendKind::Container,
        status,
        published_endpoints,
    );
    let manifest = json!({
        "handle": handle,
        "spec": {
            "tenant_id": tenant_id,
            "name": service_name,
            "backend": "container",
            "filesystem": {
                "rootfs": "/tmp/rootfs",
                "readonly": true
            },
            "process": {
                "args": ["/bin/server"],
                "env": ["PATH=/usr/bin"],
                "cwd": "/",
                "terminal": false
            },
            "resources": nimbus::SandboxResourceLimits::default(),
            "lifecycle": {
                "restart_policy": "never"
            },
            "port_bindings": [SandboxPortBinding::tcp("default", 18080, 8080)]
        },
        "conmon_layout": {
            "container_state_dir": container_dir,
            "ctr_log": container_dir.join("ctr.log"),
            "oci_log": container_dir.join("oci.log")
        },
        "last_exit_code": null,
        "shutdown_requested": false,
        "status": status
    });

    fs::write(
        container_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).expect("manifest should serialize"),
    )
    .expect("manifest should write");
}

#[derive(Debug, Clone)]
struct RefreshingInspectBackend {
    state_root: PathBuf,
    inspected_ids: Arc<Mutex<Vec<String>>>,
}

impl RefreshingInspectBackend {
    fn new(state_root: PathBuf) -> Self {
        Self {
            state_root,
            inspected_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn inspected_ids(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.inspected_ids)
    }
}

impl SandboxBackend for RefreshingInspectBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
        let message = format!(
            "test refresh backend expects inspect only, not bare spec {}",
            spec.name
        );
        Box::pin(async move { Err(SandboxError::InvalidSpec { message }) })
    }

    fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
        let message = format!(
            "test refresh backend expects inspect only, not image launch {}",
            launch.spec.name
        );
        Box::pin(async move { Err(SandboxError::InvalidSpec { message }) })
    }

    fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
        let message = format!(
            "test refresh backend expects inspect only, not build launch {}",
            launch.spec.name
        );
        Box::pin(async move { Err(SandboxError::InvalidSpec { message }) })
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
        let state_root = self.state_root.clone();
        let sandbox_id = id.clone();
        let inspected_ids = Arc::clone(&self.inspected_ids);
        Box::pin(async move {
            inspected_ids
                .lock()
                .expect("lock should acquire")
                .push(sandbox_id.as_str().to_owned());
            let endpoints = vec![PublishedEndpoint::new(
                "default",
                PublishedEndpointProtocol::Tcp,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18080),
            )];
            write_container_manifest(
                &state_root,
                sandbox_id.as_str(),
                "svc-demo",
                "demo",
                SandboxStatus::Ready,
                endpoints.clone(),
            );
            Ok(Some(SandboxHandle::new(
                sandbox_id,
                "demo",
                SandboxBackendKind::Container,
                SandboxStatus::Ready,
                endpoints,
            )))
        })
    }

    fn stop(&self, _id: &SandboxId) -> SandboxFuture<()> {
        Box::pin(async move { Ok(()) })
    }
}
