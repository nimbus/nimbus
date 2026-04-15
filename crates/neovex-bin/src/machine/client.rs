use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use neovex::{Error, SandboxBuildLaunchSpec, SandboxHandle, SandboxId, SandboxImageLaunchSpec};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::protocol::{
    MachineApiCapabilityResponse, MachineApiErrorResponse, MachineApiHealthResponse,
    MachineApiServiceSandboxBuildStartRequest, MachineApiServiceSandboxImageStartRequest,
    MachineApiServiceSandboxInspectResponse, MachineApiServiceSandboxStartResponse,
    MachineApiServiceSandboxStopResponse,
};

const SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(test)]
const SOCKET_IO_TIMEOUT_TEST: Duration = Duration::from_secs(30);
const HEALTHZ_PATH: &str = "/healthz";
const CAPABILITIES_PATH: &str = "/v1/machine-api/capabilities";
const IMAGE_START_PATH: &str = "/v1/machine-api/service-sandboxes/image-start";
const BUILD_START_PATH: &str = "/v1/machine-api/service-sandboxes/build-start";

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct MachineApiClient {
    socket_path: PathBuf,
    io_timeout: Duration,
}

#[allow(dead_code)]
impl MachineApiClient {
    pub(crate) fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            io_timeout: SOCKET_IO_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            io_timeout: SOCKET_IO_TIMEOUT_TEST,
        }
    }

    pub(crate) fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub(crate) fn health(&self) -> Result<MachineApiHealthResponse, Error> {
        self.get_json(HEALTHZ_PATH)
    }

    pub(crate) fn capabilities(&self) -> Result<MachineApiCapabilityResponse, Error> {
        self.get_json(CAPABILITIES_PATH)
    }

    pub(crate) fn start_service_sandbox_from_image(
        &self,
        launch: SandboxImageLaunchSpec,
    ) -> Result<SandboxHandle, Error> {
        self.post_json(
            IMAGE_START_PATH,
            &MachineApiServiceSandboxImageStartRequest { launch },
        )
        .map(|response: MachineApiServiceSandboxStartResponse| response.handle)
    }

    pub(crate) fn start_service_sandbox_from_build(
        &self,
        launch: SandboxBuildLaunchSpec,
    ) -> Result<SandboxHandle, Error> {
        self.post_json(
            BUILD_START_PATH,
            &MachineApiServiceSandboxBuildStartRequest { launch },
        )
        .map(|response: MachineApiServiceSandboxStartResponse| response.handle)
    }

    pub(crate) fn inspect_service_sandbox(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<Option<SandboxHandle>, Error> {
        self.get_json::<MachineApiServiceSandboxInspectResponse>(&format!(
            "/v1/machine-api/service-sandboxes/{}",
            sandbox_id.as_str()
        ))
        .map(|response| response.handle)
    }

    pub(crate) fn stop_service_sandbox(&self, sandbox_id: &SandboxId) -> Result<(), Error> {
        let response = self.post_empty::<MachineApiServiceSandboxStopResponse>(&format!(
            "/v1/machine-api/service-sandboxes/{}/stop",
            sandbox_id.as_str()
        ))?;
        if response.stopped {
            Ok(())
        } else {
            Err(Error::Internal(format!(
                "machine API stop acknowledged false for sandbox {}",
                sandbox_id
            )))
        }
    }

    fn get_json<T>(&self, path: &str) -> Result<T, Error>
    where
        T: DeserializeOwned,
    {
        let response =
            read_unix_http_request(&self.socket_path, "GET", path, None, self.io_timeout)?;
        let body = parse_http_json_body(&response, &self.socket_path, path)?;
        serde_json::from_slice(body).map_err(|error| {
            Error::Internal(format!(
                "failed to decode machine API response from {}{}: {error}",
                self.socket_path.display(),
                path
            ))
        })
    }

    fn post_json<T, B>(&self, path: &str, body: &B) -> Result<T, Error>
    where
        T: DeserializeOwned,
        B: Serialize,
    {
        let encoded = serde_json::to_vec(body).map_err(|error| {
            Error::Internal(format!(
                "failed to encode machine API request body for {}{}: {error}",
                self.socket_path.display(),
                path
            ))
        })?;
        let response = read_unix_http_request(
            &self.socket_path,
            "POST",
            path,
            Some(&encoded),
            self.io_timeout,
        )?;
        let body = parse_http_json_body(&response, &self.socket_path, path)?;
        serde_json::from_slice(body).map_err(|error| {
            Error::Internal(format!(
                "failed to decode machine API response from {}{}: {error}",
                self.socket_path.display(),
                path
            ))
        })
    }

    fn post_empty<T>(&self, path: &str) -> Result<T, Error>
    where
        T: DeserializeOwned,
    {
        let response = read_unix_http_request(
            &self.socket_path,
            "POST",
            path,
            Some(b"{}"),
            self.io_timeout,
        )?;
        let body = parse_http_json_body(&response, &self.socket_path, path)?;
        serde_json::from_slice(body).map_err(|error| {
            Error::Internal(format!(
                "failed to decode machine API response from {}{}: {error}",
                self.socket_path.display(),
                path
            ))
        })
    }
}

fn read_unix_http_request(
    socket_path: &Path,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    io_timeout: Duration,
) -> Result<Vec<u8>, Error> {
    let mut stream = UnixStream::connect(socket_path).map_err(|error| {
        Error::Internal(format!(
            "failed to connect to machine API socket {}: {error}",
            socket_path.display()
        ))
    })?;
    stream.set_read_timeout(Some(io_timeout)).map_err(|error| {
        Error::Internal(format!(
            "failed to configure machine API socket timeout {}: {error}",
            socket_path.display()
        ))
    })?;
    let body = body.unwrap_or_default();
    let mut request = format!("{method} {path} HTTP/1.0\r\nHost: localhost\r\n");
    if !body.is_empty() {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).map_err(|error| {
        Error::Internal(format!(
            "failed to send machine API request to {}{}: {error}",
            socket_path.display(),
            path
        ))
    })?;
    if !body.is_empty() {
        stream.write_all(body).map_err(|error| {
            Error::Internal(format!(
                "failed to send machine API request body to {}{}: {error}",
                socket_path.display(),
                path
            ))
        })?;
    }

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
            Err(error) => {
                return Err(Error::Internal(format!(
                    "failed to read machine API response from {}{}: {error}",
                    socket_path.display(),
                    path
                )));
            }
        }
    }

    if response.is_empty() {
        return Err(Error::Internal(format!(
            "machine API response from {}{} was empty",
            socket_path.display(),
            path
        )));
    }

    Ok(response)
}

fn parse_http_json_body<'a>(
    response: &'a [u8],
    socket_path: &Path,
    path: &str,
) -> Result<&'a [u8], Error> {
    let response_text = String::from_utf8_lossy(response);
    let status_line = response_text.lines().next().unwrap_or("<empty-response>");
    let body_offset = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .ok_or_else(|| {
            Error::Internal(format!(
                "machine API response from {}{} did not contain an HTTP body",
                socket_path.display(),
                path
            ))
        })?;
    let body = &response[body_offset..];
    if !status_line.contains("200 OK") {
        if let Ok(error_body) = serde_json::from_slice::<MachineApiErrorResponse>(body) {
            return Err(Error::Internal(format!(
                "machine API request {}{} failed: {}",
                socket_path.display(),
                path,
                error_body.error
            )));
        }
        return Err(Error::Internal(format!(
            "machine API request {}{} did not return 200 OK: {status_line}",
            socket_path.display(),
            path
        )));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use neovex::{
        PublishedEndpoint, PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind,
        SandboxBuildLaunchSpec, SandboxError, SandboxFilesystemSpec, SandboxHandle, SandboxId,
        SandboxImageLaunchSpec, SandboxPortBinding, SandboxProcessSpec, SandboxSpec, SandboxStatus,
        TenantId,
    };
    use neovex_sandbox::SandboxFuture;
    use tempfile::{Builder, TempDir};

    use super::MachineApiClient;
    use crate::machine::api::{
        MachineApiListenMode, MachineApiState, bind_direct_listener, serve_machine_api,
    };
    use crate::machine::protocol::{MachineApiHealthResponse, MachineApiServiceExecutionMode};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn client_reads_health_and_capabilities_from_machine_api_socket() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("neovex.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
            service_backend: None,
            machine_port_forwarder: None,
        };
        write_fake_runtime_binaries(temp_dir.path());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));
        let client = MachineApiClient::new_for_test(socket_path);

        let health = wait_for_health(&client);
        assert_eq!(health.status, "ok");
        assert_eq!(health.role, "guest-machine-api");
        assert_eq!(health.protocol_version, "v1alpha1");
        assert_eq!(health.listen_mode, "direct-socket");
        assert!(health.control_data_dir.ends_with("/control"));

        let capabilities = client
            .capabilities()
            .expect("capabilities should decode cleanly");
        assert_eq!(capabilities.protocol_version, "v1alpha1");
        assert!(!capabilities.service_execution_ready);
        assert_eq!(
            capabilities.service_execution_mode,
            MachineApiServiceExecutionMode::StandardContainers
        );
        assert_eq!(
            capabilities.supported_service_backends,
            vec![neovex::SandboxBackendKind::Container]
        );
        assert_eq!(
            capabilities.supported_operations,
            vec!["healthz".to_owned(), "capabilities".to_owned()]
        );
        assert_eq!(capabilities.required_binaries.len(), 6);
        assert_eq!(
            capabilities.service_execution_blockers,
            vec!["guest machine API does not yet expose service lifecycle operations".to_owned()]
        );

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn client_round_trips_service_sandbox_operations_when_backend_is_available() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("neovex.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
            service_backend: Some(Arc::new(StubMachineApiSandboxBackend::default())),
            machine_port_forwarder: None,
        };
        write_fake_runtime_binaries(temp_dir.path());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));
        let client = MachineApiClient::new_for_test(socket_path);
        let _ = wait_for_health(&client);

        let capabilities = client
            .capabilities()
            .expect("capabilities should decode cleanly");
        assert!(capabilities.service_execution_ready);
        assert_eq!(
            capabilities.supported_service_backends,
            vec![SandboxBackendKind::Container]
        );
        assert!(
            capabilities
                .supported_operations
                .contains(&"service-sandboxes.image-start".to_owned())
        );
        assert!(capabilities.service_execution_blockers.is_empty());

        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let image_launch =
            SandboxImageLaunchSpec::new(sample_spec(&tenant_id, "db"), "docker://busybox:latest");
        let image_handle = client
            .start_service_sandbox_from_image(image_launch)
            .expect("image-backed sandbox should start");
        assert_eq!(image_handle.name, "db");
        assert_eq!(image_handle.backend, SandboxBackendKind::Container);
        assert_eq!(image_handle.status, SandboxStatus::Ready);
        assert_eq!(image_handle.published_endpoints.len(), 1);

        let inspected = client
            .inspect_service_sandbox(&image_handle.id)
            .expect("inspect should succeed")
            .expect("started sandbox should inspect");
        assert_eq!(inspected, image_handle);

        client
            .stop_service_sandbox(&image_handle.id)
            .expect("stop should succeed");
        assert!(
            client
                .inspect_service_sandbox(&image_handle.id)
                .expect("inspect after stop should succeed")
                .is_none(),
            "stopped sandbox should disappear from inspect"
        );

        let build_launch = SandboxBuildLaunchSpec::new(
            sample_spec(&tenant_id, "api"),
            "api-image",
            "/Users/jack/src/github.com/agentstation/neovex/Dockerfile",
            "/Users/jack/src/github.com/agentstation/neovex",
        );
        let build_handle = client
            .start_service_sandbox_from_build(build_launch)
            .expect("build-backed sandbox should start");
        assert_eq!(build_handle.name, "api");

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[test]
    fn client_reports_missing_socket_cleanly() {
        let client = MachineApiClient::new("/tmp/neovex-missing.sock");
        let error = client
            .health()
            .expect_err("missing socket should fail cleanly");
        assert!(
            error
                .to_string()
                .contains("failed to connect to machine API socket"),
            "{error}"
        );
    }

    fn wait_for_health(client: &MachineApiClient) -> MachineApiHealthResponse {
        let start = std::time::Instant::now();
        loop {
            match client.health() {
                Ok(response) => return response,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => panic!("machine API health never became reachable: {error}"),
            }
        }
    }

    fn write_fake_runtime_binaries(dir: &std::path::Path) {
        for binary in [
            "buildah",
            "conmon",
            "crun",
            "netavark",
            "aardvark-dns",
            "fuse-overlayfs",
        ] {
            let path = dir.join(binary);
            std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("fake runtime binary should write");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                let permissions = std::fs::Permissions::from_mode(0o755);
                std::fs::set_permissions(&path, permissions)
                    .expect("fake runtime binary should be executable");
            }
        }
    }

    fn sample_spec(tenant_id: &TenantId, name: &str) -> SandboxSpec {
        SandboxSpec::new(
            tenant_id.clone(),
            name,
            SandboxBackendKind::Container,
            SandboxFilesystemSpec::new("/"),
            SandboxProcessSpec::new(["sleep", "60"]),
        )
        .with_port_binding(SandboxPortBinding::new(
            "http",
            PublishedEndpointProtocol::Http,
            18080,
            8080,
        ))
    }

    fn short_socket_tempdir() -> TempDir {
        Builder::new()
            .prefix("neovex-mac-")
            .tempdir_in("/tmp")
            .expect("short temp dir should exist")
    }

    #[derive(Default)]
    struct StubMachineApiSandboxBackend {
        next_id: AtomicUsize,
        handles: Mutex<BTreeMap<String, SandboxHandle>>,
    }

    impl StubMachineApiSandboxBackend {
        fn start_with_spec(&self, spec: &SandboxSpec) -> SandboxHandle {
            let sequence = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
            let sandbox_id = SandboxId::new(format!("{}-{sequence}", spec.name));
            let endpoints = spec
                .port_bindings
                .iter()
                .map(|binding| {
                    PublishedEndpoint::new(
                        binding.name.clone(),
                        binding.protocol,
                        SocketAddr::new(
                            IpAddr::V4(Ipv4Addr::LOCALHOST),
                            binding.host_socket_addr().port(),
                        ),
                    )
                })
                .collect::<Vec<_>>();
            let handle = SandboxHandle::new(
                sandbox_id.clone(),
                spec.name.clone(),
                SandboxBackendKind::Container,
                SandboxStatus::Ready,
                endpoints,
            );
            self.handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .insert(sandbox_id.as_str().to_owned(), handle.clone());
            handle
        }
    }

    impl SandboxBackend for StubMachineApiSandboxBackend {
        fn kind(&self) -> SandboxBackendKind {
            SandboxBackendKind::Container
        }

        fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
            let message = format!(
                "stub backend expects image/build launch, not bare spec {}",
                spec.name
            );
            Box::pin(async move { Err(SandboxError::InvalidSpec { message }) })
        }

        fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
            let handle = self.start_with_spec(&launch.spec);
            Box::pin(async move { Ok(handle) })
        }

        fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
            let handle = self.start_with_spec(&launch.spec);
            Box::pin(async move { Ok(handle) })
        }

        fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
            let handle = self
                .handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .get(id.as_str())
                .cloned();
            Box::pin(async move { Ok(handle) })
        }

        fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
            self.handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .remove(id.as_str());
            Box::pin(async move { Ok(()) })
        }
    }
}
