use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nimbus::{Error, SandboxHandle, SandboxId, SandboxRootSpec, SandboxSpec, TenantId};
use serde::Serialize;
use serde::de::DeserializeOwned;

use nimbus_machine::api::{
    MACHINE_API_BOOTC_ROLLBACK_PATH, MACHINE_API_BOOTC_STATUS_PATH, MACHINE_API_BOOTC_SWITCH_PATH,
    MACHINE_API_BOOTC_UPGRADE_PATH, MACHINE_API_CAPABILITIES_PATH, MACHINE_API_HEALTH_PATH,
    MACHINE_API_SERVICE_SANDBOX_BUILD_START_PATH, MACHINE_API_SERVICE_SANDBOX_IMAGE_START_PATH,
    MachineApiBootcOperationResponse, MachineApiBootcRollbackRequest,
    MachineApiBootcStatusResponse, MachineApiBootcSwitchRequest, MachineApiBootcUpgradeRequest,
    MachineApiCapabilityResponse, MachineApiErrorResponse, MachineApiHealthResponse,
    MachineApiServiceProcessSnapshot, MachineApiServiceProcessSnapshotResponse,
    MachineApiServiceSandboxBuildStartRequest, MachineApiServiceSandboxImageStartRequest,
    MachineApiServiceSandboxInspectResponse, MachineApiServiceSandboxListResponse,
    MachineApiServiceSandboxLogChunkResponse, MachineApiServiceSandboxLookupResponse,
    MachineApiServiceSandboxStartResponse, MachineApiServiceSandboxStopResponse,
    MachineApiServiceSandboxSummary, PROTOCOL_VERSION, machine_api_current_service_sandbox_path,
    machine_api_service_sandbox_list_path, machine_api_service_sandbox_logs_path,
    machine_api_service_sandbox_path, machine_api_service_sandbox_process_snapshot_path,
    machine_api_service_sandbox_stop_path,
};

const SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(2);
const SOCKET_MUTATION_IO_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
const SOCKET_IO_TIMEOUT_TEST: Duration = Duration::from_secs(30);
const LOCAL_GUEST_BINARY_HELP_TEXT: &str = "set `NIMBUS_MACHINE_GUEST_BINARY` only when you intentionally need a local Linux guest binary override";

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
        self.get_json(MACHINE_API_HEALTH_PATH)
    }

    pub(crate) fn capabilities(&self) -> Result<MachineApiCapabilityResponse, Error> {
        let response = read_unix_http_request(
            &self.socket_path,
            "GET",
            MACHINE_API_CAPABILITIES_PATH,
            None,
            self.io_timeout,
        )?;
        let body =
            parse_http_json_body(&response, &self.socket_path, MACHINE_API_CAPABILITIES_PATH)?;
        serde_json::from_slice(body)
            .map_err(|error| describe_capability_decode_error(&self.socket_path, body, error))
    }

    pub(crate) fn bootc_status(&self) -> Result<MachineApiBootcStatusResponse, Error> {
        self.get_json(MACHINE_API_BOOTC_STATUS_PATH)
    }

    pub(crate) fn bootc_switch(
        &self,
        request: MachineApiBootcSwitchRequest,
    ) -> Result<MachineApiBootcOperationResponse, Error> {
        self.post_json(MACHINE_API_BOOTC_SWITCH_PATH, &request)
    }

    pub(crate) fn bootc_upgrade(
        &self,
        request: MachineApiBootcUpgradeRequest,
    ) -> Result<MachineApiBootcOperationResponse, Error> {
        self.post_json(MACHINE_API_BOOTC_UPGRADE_PATH, &request)
    }

    pub(crate) fn bootc_rollback(
        &self,
        request: MachineApiBootcRollbackRequest,
    ) -> Result<MachineApiBootcOperationResponse, Error> {
        self.post_json(MACHINE_API_BOOTC_ROLLBACK_PATH, &request)
    }

    pub(crate) fn start_service_sandbox_from_image(
        &self,
        spec: SandboxSpec,
    ) -> Result<SandboxHandle, Error> {
        self.post_json(
            MACHINE_API_SERVICE_SANDBOX_IMAGE_START_PATH,
            &MachineApiServiceSandboxImageStartRequest { spec },
        )
        .map(|response: MachineApiServiceSandboxStartResponse| response.handle)
    }

    pub(crate) fn start_service_sandbox_from_build(
        &self,
        spec: SandboxSpec,
    ) -> Result<SandboxHandle, Error> {
        let spec = normalize_guest_visible_build_spec(spec);
        self.post_json(
            MACHINE_API_SERVICE_SANDBOX_BUILD_START_PATH,
            &MachineApiServiceSandboxBuildStartRequest { spec },
        )
        .map(|response: MachineApiServiceSandboxStartResponse| response.handle)
    }

    pub(crate) fn inspect_service_sandbox(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<Option<SandboxHandle>, Error> {
        self.get_json::<MachineApiServiceSandboxInspectResponse>(&machine_api_service_sandbox_path(
            sandbox_id.as_str(),
        ))
        .map(|response| response.handle)
    }

    pub(crate) fn stop_service_sandbox(&self, sandbox_id: &SandboxId) -> Result<(), Error> {
        let response = self.post_empty::<MachineApiServiceSandboxStopResponse>(
            &machine_api_service_sandbox_stop_path(sandbox_id.as_str()),
        )?;
        if response.stopped {
            Ok(())
        } else {
            Err(Error::Internal(format!(
                "machine API stop acknowledged false for sandbox {}",
                sandbox_id
            )))
        }
    }

    pub(crate) fn list_service_sandboxes(
        &self,
        tenant_id: Option<&TenantId>,
    ) -> Result<Vec<MachineApiServiceSandboxSummary>, Error> {
        let path = machine_api_service_sandbox_list_path(tenant_id.map(TenantId::as_str));
        self.get_json::<MachineApiServiceSandboxListResponse>(&path)
            .map(|response| response.sandboxes)
    }

    pub(crate) fn inspect_current_service_sandbox(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<MachineApiServiceSandboxLookupResponse, Error> {
        self.get_json(&machine_api_current_service_sandbox_path(
            tenant_id.as_str(),
            service_name,
        ))
    }

    pub(crate) fn read_service_sandbox_log_chunk(
        &self,
        sandbox_id: &SandboxId,
        offset: u64,
    ) -> Result<MachineApiServiceSandboxLogChunkResponse, Error> {
        self.get_json(&machine_api_service_sandbox_logs_path(
            sandbox_id.as_str(),
            offset,
        ))
    }

    pub(crate) fn service_sandbox_process_snapshot(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<MachineApiServiceProcessSnapshot, Error> {
        self.get_json::<MachineApiServiceProcessSnapshotResponse>(
            &machine_api_service_sandbox_process_snapshot_path(sandbox_id.as_str()),
        )
        .map(|response| response.snapshot)
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
            SOCKET_MUTATION_IO_TIMEOUT,
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
            None,
            SOCKET_MUTATION_IO_TIMEOUT,
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

fn normalize_guest_visible_build_spec(mut spec: SandboxSpec) -> SandboxSpec {
    if let SandboxRootSpec::OciImage(image) = &mut spec.root
        && let nimbus::SandboxOciImageSource::Build(build) = &mut image.source
    {
        build.dockerfile_path = normalize_guest_visible_host_path(&build.dockerfile_path);
        build.context_path = normalize_guest_visible_host_path(&build.context_path);
    }
    spec
}

fn normalize_guest_visible_host_path(path: &Path) -> PathBuf {
    if !cfg!(target_os = "macos") || !path.is_absolute() {
        return path.to_path_buf();
    }

    if let Ok(canonical) = fs::canonicalize(path) {
        return canonical;
    }

    if path == Path::new("/tmp") {
        return PathBuf::from("/private/tmp");
    }

    if let Ok(relative) = path.strip_prefix("/tmp/") {
        return PathBuf::from("/private/tmp").join(relative);
    }

    path.to_path_buf()
}

fn describe_capability_decode_error(
    socket_path: &Path,
    body: &[u8],
    error: serde_json::Error,
) -> Error {
    let reported_protocol = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("protocol_version")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    match reported_protocol {
        Some(protocol_version) if protocol_version != PROTOCOL_VERSION => Error::Internal(format!(
            "guest machine API protocol mismatch at {}{}: host expects {}, guest reported {}. Re-sync a matching guest nimbus binary and retry ({LOCAL_GUEST_BINARY_HELP_TEXT})",
            socket_path.display(),
            MACHINE_API_CAPABILITIES_PATH,
            PROTOCOL_VERSION,
            protocol_version
        )),
        _ => Error::Internal(format!(
            "failed to decode machine API response from {}{}: {error}",
            socket_path.display(),
            MACHINE_API_CAPABILITIES_PATH
        )),
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
    } else if method == "POST" {
        request.push_str("Content-Length: 0\r\n");
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
            Ok(read) => {
                response.extend_from_slice(&chunk[..read]);
                // A satisfied Content-Length means the message is complete
                // — break regardless of `Connection: close`. The close
                // directive announces the server WILL close; it does not
                // require draining to EOF once the length is known, and
                // waiting here deadlocks against one-shot servers that
                // keep the stream open while accepting their next
                // connection. EOF delimits the body only when no
                // Content-Length was sent (the `Ok(0)` arm).
                if let Some(expected_len) =
                    expected_http_response_len(&response, socket_path, path)?
                    && response.len() >= expected_len
                {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err(Error::Internal(format!(
                    "timed out reading machine API response from {}{} after {} bytes: {error}",
                    socket_path.display(),
                    path,
                    response.len()
                )));
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

    if let Some(expected_len) = expected_http_response_len(&response, socket_path, path)?
        && response.len() < expected_len
    {
        return Err(Error::Internal(format!(
            "machine API response from {}{} closed after {} bytes before the declared {} byte response completed",
            socket_path.display(),
            path,
            response.len(),
            expected_len
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
    let body_offset = http_response_body_offset(response).ok_or_else(|| {
        Error::Internal(format!(
            "machine API response from {}{} did not contain an HTTP body",
            socket_path.display(),
            path
        ))
    })?;
    let body = &response[body_offset..];
    let status_code = parse_http_status_code(status_line);
    if status_code != Some(200) {
        if let Ok(error_body) = serde_json::from_slice::<MachineApiErrorResponse>(body) {
            return Err(machine_api_status_error(
                status_code,
                format!(
                    "machine API request {}{} failed: {}",
                    socket_path.display(),
                    path,
                    error_body.error
                ),
            ));
        }
        return Err(machine_api_status_error(
            status_code,
            format!(
                "machine API request {}{} did not return 200 OK: {status_line}",
                socket_path.display(),
                path
            ),
        ));
    }
    Ok(body)
}

fn http_response_body_offset(response: &[u8]) -> Option<usize> {
    response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn expected_http_response_len(
    response: &[u8],
    socket_path: &Path,
    path: &str,
) -> Result<Option<usize>, Error> {
    let Some(body_offset) = http_response_body_offset(response) else {
        return Ok(None);
    };
    let headers = String::from_utf8_lossy(&response[..body_offset]);
    let mut content_length = None;
    for line in headers.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        let parsed = value.trim().parse::<usize>().map_err(|error| {
            Error::Internal(format!(
                "machine API response from {}{} had invalid Content-Length `{}`: {error}",
                socket_path.display(),
                path,
                value.trim()
            ))
        })?;
        if let Some(previous) = content_length
            && previous != parsed
        {
            return Err(Error::Internal(format!(
                "machine API response from {}{} had conflicting Content-Length values {previous} and {parsed}",
                socket_path.display(),
                path
            )));
        }
        content_length = Some(parsed);
    }
    content_length
        .map(|len| {
            body_offset.checked_add(len).ok_or_else(|| {
                Error::Internal(format!(
                    "machine API response from {}{} declared an oversized Content-Length",
                    socket_path.display(),
                    path
                ))
            })
        })
        .transpose()
}

fn parse_http_status_code(status_line: &str) -> Option<u16> {
    let mut parts = status_line.split_ascii_whitespace();
    let _http_version = parts.next()?;
    parts.next()?.parse::<u16>().ok()
}

fn machine_api_status_error(status_code: Option<u16>, message: String) -> Error {
    match status_code {
        Some(400 | 422) => Error::InvalidInput(message),
        Some(401 | 403) => Error::PermissionDenied(message),
        Some(404) => Error::NotFound(message),
        Some(409) => Error::Conflict(message),
        Some(412) => Error::PreconditionFailed(message),
        Some(429) => Error::ResourceExhausted(message),
        _ => Error::Internal(message),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::os::unix::net::UnixListener as StdUnixListener;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use nimbus::{
        Error, PublishedEndpoint, PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind,
        SandboxHandle, SandboxId, SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec,
        SandboxRootSpec, SandboxSpec, SandboxStatus, TenantId,
    };
    use nimbus_sandbox::SandboxFuture;
    use nimbus_sandbox::backends::container::{
        ContainerSandboxBackend, ContainerSandboxBackendConfig, ContainerStartMode,
    };
    use tempfile::{Builder, TempDir};

    use super::{
        MachineApiClient, normalize_guest_visible_build_spec, normalize_guest_visible_host_path,
    };
    use crate::machine::api::{
        MachineApiListenMode, MachineApiState, bind_direct_listener,
        default_guest_helper_binary_dirs, machine_api_node_workload_facade_from_sandbox_backend,
        serve_machine_api,
    };
    use nimbus_machine::api::{
        MachineApiHealthResponse, MachineApiServiceExecutionDriver, MachineApiServiceExecutionMode,
        PROTOCOL_VERSION, machine_api_path_segment, machine_api_query_path,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn client_reads_health_and_capabilities_from_machine_api_socket() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("nimbus.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
            helper_binary_dirs: default_guest_helper_binary_dirs(),
            service_workloads: None,
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
        assert_eq!(health.protocol_version, PROTOCOL_VERSION);
        assert_eq!(health.listen_mode, "direct-socket");
        assert!(health.control_data_dir.ends_with("/control"));

        let capabilities = client
            .capabilities()
            .expect("capabilities should decode cleanly");
        assert_eq!(capabilities.protocol_version, PROTOCOL_VERSION);
        assert!(!capabilities.service_execution_ready);
        assert_eq!(
            capabilities.service_execution_mode,
            MachineApiServiceExecutionMode::StandardContainers
        );
        assert_eq!(
            capabilities.service_execution_driver,
            MachineApiServiceExecutionDriver::Unavailable
        );
        assert_eq!(
            capabilities.supported_service_backends,
            vec![nimbus::SandboxBackendKind::Container]
        );
        assert_eq!(
            capabilities.supported_operations,
            vec![
                "healthz".to_owned(),
                "capabilities".to_owned(),
                "os.bootc.status".to_owned(),
                "os.bootc.switch".to_owned(),
                "os.bootc.upgrade".to_owned(),
                "os.bootc.rollback".to_owned(),
            ]
        );
        assert_eq!(
            capabilities
                .binary_statuses
                .iter()
                .map(|status| status.name.as_str())
                .collect::<Vec<_>>(),
            vec!["bootc", "conmon", "crun", "netavark", "aardvark-dns"]
        );
        assert!(
            capabilities
                .binary_statuses
                .iter()
                .all(|status| status.present)
        );
        assert_eq!(
            capabilities.service_execution_blockers,
            vec!["guest machine API does not yet expose service lifecycle operations".to_owned()]
        );
        assert!(
            capabilities
                .operation_statuses
                .iter()
                .any(|status| status.name == "service-sandboxes.build-start" && !status.available)
        );

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[test]
    fn client_reports_guest_protocol_mismatch_cleanly() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("nimbus.sock");
        let listener = StdUnixListener::bind(&socket_path).expect("listener should bind");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("server should accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = serde_json::json!({
                "protocol_version": "v1alpha1",
                "service_execution_ready": true,
                "service_execution_mode": "standard_containers",
                "service_execution_driver": "guest_node_agent_systemd_transient_unit",
                "supported_service_backends": ["container"],
                "supported_operations": ["healthz", "capabilities"],
                "service_execution_blockers": [],
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("server should write capabilities response");
        });
        let client = MachineApiClient::new_for_test(socket_path);

        let error = client
            .capabilities()
            .expect_err("older guest protocol should fail clearly");

        let message = error.to_string();
        assert!(
            message.contains("guest machine API protocol mismatch"),
            "{message}"
        );
        assert!(message.contains(PROTOCOL_VERSION), "{message}");
        assert!(message.contains("v1alpha1"), "{message}");
        assert!(message.contains("NIMBUS_MACHINE_GUEST_BINARY"), "{message}");
        assert!(
            message.contains("local Linux guest binary override"),
            "{message}"
        );

        server.join().expect("server should join");
    }

    #[test]
    fn client_preserves_machine_api_bad_request_as_invalid_input() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("nimbus.sock");
        let listener = StdUnixListener::bind(&socket_path).expect("listener should bind");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("server should accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = serde_json::json!({
                "error": "service-sandboxes.image-start requires OCI image reference",
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("server should write error response");
        });
        let client = MachineApiClient::new_for_test(socket_path);

        let error = client
            .capabilities()
            .expect_err("guest contract rejection should stay typed");

        assert!(
            matches!(
                &error,
                Error::InvalidInput(message)
                    if message.contains("machine API request")
                        && message.contains("requires OCI image reference")
            ),
            "400 machine API errors should remain InvalidInput: {error}"
        );
        server.join().expect("server should join");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn client_round_trips_service_sandbox_operations_when_backend_is_available() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("nimbus.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
            helper_binary_dirs: default_guest_helper_binary_dirs(),
            service_workloads: Some(machine_api_node_workload_facade_from_sandbox_backend(
                Arc::new(StubMachineApiSandboxBackend::default()),
            )),
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
            capabilities.service_execution_driver,
            MachineApiServiceExecutionDriver::GuestNodeAgentSystemdTransientUnit
        );
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
        assert!(
            capabilities
                .operation_statuses
                .iter()
                .any(|status| status.name == "service-sandboxes.build-start" && status.available)
        );

        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let image_handle = client
            .start_service_sandbox_from_image(image_spec(
                &tenant_id,
                "db",
                "docker://busybox:latest",
            ))
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

        let build_handle = client
            .start_service_sandbox_from_build(build_spec(
                &tenant_id,
                "api",
                "api-image",
                "/Users/jack/src/github.com/nimbus/nimbus/Dockerfile",
                "/Users/jack/src/github.com/nimbus/nimbus",
            ))
            .expect("build-backed sandbox should start");
        assert_eq!(build_handle.name, "api");

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn client_reads_list_current_logs_and_process_snapshot_from_machine_api() {
        let temp_dir = short_socket_tempdir();
        let control_data_dir = temp_dir.path().join("control");
        let manifest_state_root = control_data_dir
            .join("service-sandboxes")
            .join("container")
            .join("state");
        let socket_path = temp_dir.path().join("nimbus.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let mut backend_config = ContainerSandboxBackendConfig::under_root(
            control_data_dir.join("service-sandboxes").join("container"),
        );
        backend_config.start_mode = ContainerStartMode::PlanOnly;
        let state = MachineApiState {
            control_data_dir,
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
            helper_binary_dirs: default_guest_helper_binary_dirs(),
            service_workloads: Some(machine_api_node_workload_facade_from_sandbox_backend(
                Arc::new(ContainerSandboxBackend::new(backend_config)),
            )),
            machine_port_forwarder: None,
        };
        write_fake_runtime_binaries(temp_dir.path());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));
        let client = MachineApiClient::new_for_test(socket_path);
        let _ = wait_for_health(&client);

        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let sandbox_id = SandboxId::new("db-01aaa");
        let container_dir = write_container_manifest(
            manifest_state_root.as_path(),
            sandbox_id.as_str(),
            tenant_id.as_str(),
            "db",
            SandboxStatus::Ready,
        );
        fs::write(container_dir.join("ctr.log"), "guest log line\n")
            .expect("guest ctr.log should write");
        fs::write(container_dir.join("pidfile"), "2002\n").expect("pidfile should write");
        fs::write(container_dir.join("conmon.pid"), "1001\n").expect("conmon pidfile should write");

        let summaries = client
            .list_service_sandboxes(Some(&tenant_id))
            .expect("sandbox list should succeed");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].service_name, "db");
        assert_eq!(summaries[0].sandbox_id, sandbox_id);

        let current = client
            .inspect_current_service_sandbox(&tenant_id, "db")
            .expect("current sandbox lookup should succeed")
            .details
            .expect("current sandbox should resolve");
        assert_eq!(current.summary.sandbox_id, sandbox_id);
        assert!(current.log_paths.ctr_log.ends_with("ctr.log"));

        let logs = client
            .read_service_sandbox_log_chunk(&sandbox_id, 0)
            .expect("log chunk should read");
        assert_eq!(logs.chunk, "guest log line\n");
        assert_eq!(logs.next_offset, 15);

        let snapshot = client
            .service_sandbox_process_snapshot(&sandbox_id)
            .expect("process snapshot should read");
        assert_eq!(snapshot.runtime_pid, Some(2002));
        assert_eq!(snapshot.conmon_pid, Some(1001));

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn client_encodes_current_service_lookup_query_values() {
        let temp_dir = short_socket_tempdir();
        let control_data_dir = temp_dir.path().join("control");
        let manifest_state_root = control_data_dir
            .join("service-sandboxes")
            .join("container")
            .join("state");
        let socket_path = temp_dir.path().join("nimbus.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let mut backend_config = ContainerSandboxBackendConfig::under_root(
            control_data_dir.join("service-sandboxes").join("container"),
        );
        backend_config.start_mode = ContainerStartMode::PlanOnly;
        let state = MachineApiState {
            control_data_dir,
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
            helper_binary_dirs: default_guest_helper_binary_dirs(),
            service_workloads: Some(machine_api_node_workload_facade_from_sandbox_backend(
                Arc::new(ContainerSandboxBackend::new(backend_config)),
            )),
            machine_port_forwarder: None,
        };
        write_fake_runtime_binaries(temp_dir.path());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));
        let client = MachineApiClient::new_for_test(socket_path);
        let _ = wait_for_health(&client);

        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let sandbox_id = SandboxId::new("db-special-01aaa");
        let service_name = "db & cache=1/path";
        write_container_manifest(
            manifest_state_root.as_path(),
            sandbox_id.as_str(),
            tenant_id.as_str(),
            service_name,
            SandboxStatus::Ready,
        );

        let current = client
            .inspect_current_service_sandbox(&tenant_id, service_name)
            .expect("encoded current sandbox lookup should succeed")
            .details
            .expect("encoded current sandbox should resolve");
        assert_eq!(current.summary.sandbox_id, sandbox_id);
        assert_eq!(current.summary.service_name, service_name);

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[test]
    fn machine_api_query_path_percent_encodes_query_delimiters() {
        assert_eq!(
            machine_api_query_path(
                "/v1/machine-api/service-sandboxes/current",
                &[
                    ("tenant_id", "tenant"),
                    ("service_name", "db & cache=1/path☁")
                ]
            ),
            "/v1/machine-api/service-sandboxes/current?tenant_id=tenant&service_name=db%20%26%20cache%3D1%2Fpath%E2%98%81"
        );
    }

    #[test]
    fn client_reports_missing_socket_cleanly() {
        let client = MachineApiClient::new("/tmp/nimbus-missing.sock");
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

    #[test]
    fn client_accepts_complete_content_length_response_without_socket_eof() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("nimbus.sock");
        let listener = StdUnixListener::bind(&socket_path).expect("listener should bind");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("server should accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = serde_json::json!({
                "status": "ok",
                "role": "guest-machine-api",
                "protocol_version": PROTOCOL_VERSION,
                "listen_mode": "direct-socket",
                "control_data_dir": "/tmp/nimbus-control",
            })
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("server should write complete response");
            std::thread::sleep(Duration::from_millis(200));
        });
        let client = MachineApiClient {
            socket_path,
            io_timeout: Duration::from_millis(50),
        };

        let health = client
            .health()
            .expect("complete Content-Length response should not require EOF");

        assert_eq!(health.status, "ok");
        assert_eq!(health.protocol_version, PROTOCOL_VERSION);
        server.join().expect("server should join");
    }

    #[test]
    fn client_reports_timeout_before_declared_response_body_completes() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("nimbus.sock");
        let listener = StdUnixListener::bind(&socket_path).expect("listener should bind");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("server should accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 64\r\n\r\n{\"status\":\"ok\"",
                )
                .expect("server should write partial response");
            std::thread::sleep(Duration::from_millis(200));
        });
        let client = MachineApiClient {
            socket_path,
            io_timeout: Duration::from_millis(50),
        };

        let error = client
            .health()
            .expect_err("partial response should time out before successful parse");

        let message = error.to_string();
        assert!(
            message.contains("timed out reading machine API response"),
            "{message}"
        );
        assert!(
            !message.contains("failed to decode machine API response"),
            "{message}"
        );
        server.join().expect("server should join");
    }

    #[test]
    fn client_rejects_eof_before_declared_response_body_completes() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("nimbus.sock");
        let listener = StdUnixListener::bind(&socket_path).expect("listener should bind");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("server should accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 64\r\n\r\n{\"status\":\"ok\"",
                )
                .expect("server should write truncated response");
        });
        let client = MachineApiClient {
            socket_path,
            io_timeout: Duration::from_secs(2),
        };

        let error = client
            .health()
            .expect_err("truncated response should not parse as a clean EOF");

        let message = error.to_string();
        assert!(
            message.contains("closed after") && message.contains("declared"),
            "{message}"
        );
        server.join().expect("server should join");
    }

    #[test]
    fn guest_visible_path_normalization_preserves_non_absolute_paths() {
        assert_eq!(
            normalize_guest_visible_host_path(Path::new("Dockerfile")),
            PathBuf::from("Dockerfile")
        );
    }

    #[test]
    fn guest_visible_path_normalization_rewrites_tmp_prefix_when_needed() {
        let path = Path::new("/tmp/nimbus-build-context/Dockerfile");
        if cfg!(target_os = "macos") {
            assert_eq!(
                normalize_guest_visible_host_path(path),
                PathBuf::from("/private/tmp/nimbus-build-context/Dockerfile")
            );
        } else {
            assert_eq!(normalize_guest_visible_host_path(path), path);
        }
    }

    #[test]
    fn guest_visible_build_spec_normalization_updates_both_paths() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let spec = normalize_guest_visible_build_spec(build_spec(
            &tenant_id,
            "api",
            "api-image",
            "/tmp/nimbus-build-context/Dockerfile",
            "/tmp/nimbus-build-context",
        ));
        let SandboxRootSpec::OciImage(image) = spec.root else {
            panic!("build spec should stay OCI-image rooted");
        };
        let nimbus::SandboxOciImageSource::Build(build) = image.source else {
            panic!("build spec should stay build-backed");
        };
        if cfg!(target_os = "macos") {
            assert_eq!(
                build.dockerfile_path,
                PathBuf::from("/private/tmp/nimbus-build-context/Dockerfile")
            );
            assert_eq!(
                build.context_path,
                PathBuf::from("/private/tmp/nimbus-build-context")
            );
        } else {
            assert_eq!(
                build.dockerfile_path,
                PathBuf::from("/tmp/nimbus-build-context/Dockerfile")
            );
            assert_eq!(
                build.context_path,
                PathBuf::from("/tmp/nimbus-build-context")
            );
        }
    }

    #[test]
    fn stop_service_sandbox_sends_a_content_length_zero_post_request() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("nimbus.sock");
        let listener = StdUnixListener::bind(&socket_path).expect("listener should bind");
        let expected_path = "/v1/machine-api/service-sandboxes/db-1/stop";
        let response_body = "{\"sandbox_id\":\"db-1\",\"stopped\":true}".to_string();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should connect");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout should set");

            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        request.extend_from_slice(&chunk[..read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        break;
                    }
                    Err(error) => panic!("request should read: {error}"),
                }
            }

            write!(
                stream,
                "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                response_body.len(),
                response_body
            )
            .expect("response should write");

            String::from_utf8(request).expect("request should be valid utf-8")
        });

        let client = MachineApiClient::new_for_test(socket_path);
        client
            .stop_service_sandbox(&SandboxId::new("db-1"))
            .expect("stop should succeed");

        let request = server.join().expect("server should join");
        assert!(
            request.starts_with(&format!("POST {expected_path} HTTP/1.0\r\n")),
            "{request}"
        );
        assert!(
            request.contains("Content-Length: 0\r\n"),
            "bodyless stop request should advertise Content-Length: 0: {request}"
        );
        assert!(
            !request.contains("{}"),
            "bodyless stop request should not send a JSON stub body: {request}"
        );
    }

    #[test]
    fn machine_api_path_segment_encodes_reserved_and_structural_characters() {
        // The common case (server-minted, all-unreserved) must pass through
        // byte-identical so real traffic is unchanged.
        assert_eq!(machine_api_path_segment("db-1"), "db-1");
        // `..` and `/` must collapse into the segment so a traversal-shaped id
        // cannot climb out of the path segment.
        assert_eq!(machine_api_path_segment("../etc"), "..%2Fetc");
        assert_eq!(machine_api_path_segment("a/b"), "a%2Fb");
        // Space and the percent literal must escape (a raw `%` would otherwise
        // be read as the start of an escape octet by the server decoder).
        assert_eq!(machine_api_path_segment("a b"), "a%20b");
        assert_eq!(machine_api_path_segment("50%off"), "50%25off");
        // `?` and `#` would start a query/fragment if left raw.
        assert_eq!(machine_api_path_segment("q?x#y"), "q%3Fx%23y");
    }

    /// Bind a one-shot Unix listener, run `drive` against a client pointed at
    /// it, and return the request line (first CRLF-terminated line) the client
    /// actually sent. The server replies with `response_body` as a 200 JSON
    /// body so the client call resolves to `Ok`.
    fn capture_machine_api_request_line(
        response_body: &str,
        drive: impl FnOnce(&MachineApiClient),
    ) -> String {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("nimbus.sock");
        let listener = StdUnixListener::bind(&socket_path).expect("listener should bind");
        let response_body = response_body.to_owned();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should connect");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout should set");

            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        request.extend_from_slice(&chunk[..read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        break;
                    }
                    Err(error) => panic!("request should read: {error}"),
                }
            }

            write!(
                stream,
                "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                response_body.len(),
                response_body
            )
            .expect("response should write");

            String::from_utf8(request).expect("request should be valid utf-8")
        });

        let client = MachineApiClient::new_for_test(socket_path);
        drive(&client);

        server.join().expect("server should join")
    }

    #[test]
    fn inspect_service_sandbox_encodes_hostile_id_into_request_path() {
        // `handle: null` keeps the body minimal while staying a valid
        // MachineApiServiceSandboxInspectResponse so the client returns Ok.
        let response_body = "{\"sandbox_id\":\"../../etc/passwd\",\"handle\":null}".to_string();
        let request = capture_machine_api_request_line(&response_body, |client| {
            client
                .inspect_service_sandbox(&SandboxId::new("../../etc/passwd"))
                .expect("inspect should succeed");
        });

        assert!(
            request.starts_with(
                "GET /v1/machine-api/service-sandboxes/..%2F..%2Fetc%2Fpasswd HTTP/1.0\r\n"
            ),
            "hostile id must collapse to one safe segment with no raw `/` or `..`: {request}"
        );
    }

    #[test]
    fn stop_log_and_ps_paths_encode_hostile_id_segment() {
        // stop: reserved space + percent literal in the id.
        let stop_request = capture_machine_api_request_line(
            "{\"sandbox_id\":\"a b%c\",\"stopped\":true}",
            |client| {
                client
                    .stop_service_sandbox(&SandboxId::new("a b%c"))
                    .expect("stop should succeed");
            },
        );
        assert!(
            stop_request
                .starts_with("POST /v1/machine-api/service-sandboxes/a%20b%25c/stop HTTP/1.0\r\n"),
            "{stop_request}"
        );

        // logs: structural `/` in the id; the literal `?offset=7` delimiter and
        // numeric offset must stay raw after the encoded segment.
        let log_request = capture_machine_api_request_line(
            "{\"sandbox_id\":\"x/y\",\"offset\":7,\"next_offset\":7,\"chunk\":\"\"}",
            |client| {
                client
                    .read_service_sandbox_log_chunk(&SandboxId::new("x/y"), 7)
                    .expect("log chunk should read");
            },
        );
        assert!(
            log_request.starts_with(
                "GET /v1/machine-api/service-sandboxes/x%2Fy/logs?offset=7 HTTP/1.0\r\n"
            ),
            "log path must encode the segment but keep ?offset=7 literal: {log_request}"
        );

        // ps: `#` in the id would otherwise begin a fragment.
        let snapshot_json = serde_json::json!({
            "snapshot": {
                "sandbox_id": "p#q",
                "tenant_id": "tenant",
                "service_name": "svc",
                "status": "ready",
                "runtime_pidfile": "/run/pidfile",
                "conmon_pidfile": "/run/conmon.pid",
                "runtime_pid": null,
                "conmon_pid": null,
                "process_rows": []
            }
        })
        .to_string();
        let ps_request = capture_machine_api_request_line(&snapshot_json, |client| {
            client
                .service_sandbox_process_snapshot(&SandboxId::new("p#q"))
                .expect("process snapshot should read");
        });
        assert!(
            ps_request.starts_with("GET /v1/machine-api/service-sandboxes/p%23q/ps HTTP/1.0\r\n"),
            "{ps_request}"
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
            "bootc",
            "buildah",
            "conmon",
            "crun",
            "netavark",
            "aardvark-dns",
            "fuse-overlayfs",
        ] {
            let path = dir.join(binary);
            crate::test_support::write_executable_stub(&path, "#!/bin/sh\nexit 0\n");
        }
    }

    fn sample_spec(tenant_id: &TenantId, name: &str) -> SandboxSpec {
        SandboxSpec::new(
            tenant_id.clone(),
            SandboxOwnerSpec::service(name),
            SandboxBackendKind::Container,
            SandboxRootSpec::rootfs("/"),
            SandboxProcessSpec::new(["sleep", "60"]),
        )
        .with_port_binding(SandboxPortBinding::new(
            "http",
            PublishedEndpointProtocol::Http,
            18080,
            8080,
        ))
    }

    fn image_spec(tenant_id: &TenantId, name: &str, image_reference: &str) -> SandboxSpec {
        let mut spec = sample_spec(tenant_id, name);
        spec.root = SandboxRootSpec::oci_image_reference(image_reference);
        spec
    }

    fn build_spec(
        tenant_id: &TenantId,
        name: &str,
        image_name: &str,
        dockerfile_path: impl Into<PathBuf>,
        context_path: impl Into<PathBuf>,
    ) -> SandboxSpec {
        let mut spec = sample_spec(tenant_id, name);
        spec.root = SandboxRootSpec::oci_image_build(image_name, dockerfile_path, context_path);
        spec
    }

    fn short_socket_tempdir() -> TempDir {
        Builder::new()
            .prefix("nimbus-mac-")
            .tempdir_in("/tmp")
            .expect("short temp dir should exist")
    }

    fn write_container_manifest(
        state_root: &Path,
        sandbox_id: &str,
        tenant_id: &str,
        service_name: &str,
        status: SandboxStatus,
    ) -> std::path::PathBuf {
        let sandbox_state_root = state_root
            .join("tenants")
            .join(tenant_id)
            .join("sandboxes")
            .join(sandbox_id)
            .join("state");
        let container_dir = sandbox_state_root.join("containers").join(sandbox_id);
        let exit_dir = state_root.join("exits");
        let persist_dir = state_root.join("persist").join(sandbox_id);
        let bundle_dir = state_root.join("bundles").join(sandbox_id);
        let network_root = state_root.join("networks");
        let run_root = network_root.join("run");
        let netns_root = network_root.join("netns");
        let container_network_dir = network_root.join("containers").join(sandbox_id);
        fs::create_dir_all(&container_dir).expect("container manifest directory should exist");
        fs::create_dir_all(&exit_dir).expect("exit directory should exist");
        fs::create_dir_all(&persist_dir).expect("persist directory should exist");
        fs::create_dir_all(&bundle_dir).expect("bundle directory should exist");
        fs::create_dir_all(&run_root).expect("network run directory should exist");
        fs::create_dir_all(&netns_root).expect("network netns directory should exist");
        fs::create_dir_all(&container_network_dir)
            .expect("container network directory should exist");
        let handle = SandboxHandle::new(
            nimbus::TenantId::new(tenant_id).expect("tenant id should parse"),
            SandboxId::new(sandbox_id),
            service_name,
            SandboxBackendKind::Container,
            status,
            vec![PublishedEndpoint::new(
                "http",
                PublishedEndpointProtocol::Tcp,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18080),
            )],
        );
        let manifest = serde_json::json!({
            "handle": handle,
            "spec": {
                "tenant_id": tenant_id,
                "owner": {
                    "kind": "service",
                    "name": service_name
                },
                "backend": "container",
                "root": {
                    "kind": "rootfs",
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
                "port_bindings": [SandboxPortBinding::tcp("http", 18080, 8080)]
            },
            "image_metadata": {},
            "launch_artifact": null,
            "bundle_layout": {
                "bundle_dir": bundle_dir,
                "config_path": bundle_dir.join("config.json")
            },
            "conmon_layout": {
                "state_root": state_root,
                "container_state_dir": container_dir,
                "exit_dir": exit_dir,
                "persist_dir": persist_dir,
                "ctr_log": container_dir.join("ctr.log"),
                "oci_log": container_dir.join("oci.log"),
                "pidfile": container_dir.join("pidfile"),
                "conmon_pidfile": container_dir.join("conmon.pid"),
                "exit_status_file": exit_dir.join(sandbox_id),
                "manifest_path": container_dir.join("manifest.json")
            },
            "network_layout": {
                "network_root": network_root,
                "run_root": run_root,
                "netns_root": netns_root,
                "container_network_dir": container_network_dir,
                "netns_path": netns_root.join(sandbox_id),
                "status_path": container_network_dir.join("status.json"),
                "ipam_state_path": run_root.join("ipam-state.json"),
                "ipam_lock_path": run_root.join("ipam.lock")
            },
            "conmon_launch": {
                "create_command": {
                    "program": "/bin/true",
                    "args": []
                },
                "state_command": {
                    "program": "/bin/true",
                    "args": []
                },
                "start_command": {
                    "program": "/bin/true",
                    "args": []
                },
                "delete_command": {
                    "program": "/bin/true",
                    "args": []
                }
            },
            "last_exit_code": null,
            "start_mode": "plan_only",
            "shutdown_requested": matches!(status, SandboxStatus::Stopped),
            "status": status
        });
        fs::write(
            container_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest should serialize"),
        )
        .expect("manifest should write");
        container_dir
    }

    #[derive(Default)]
    struct StubMachineApiSandboxBackend {
        next_id: AtomicUsize,
        handles: Mutex<BTreeMap<String, SandboxHandle>>,
    }

    impl StubMachineApiSandboxBackend {
        fn start_with_spec(&self, spec: &SandboxSpec) -> SandboxHandle {
            let sequence = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
            let service_name = spec.display_name().to_owned();
            let sandbox_id = SandboxId::new(format!("{service_name}-{sequence}"));
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
                spec.tenant_id.clone(),
                sandbox_id.clone(),
                service_name,
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
            let handle = self.start_with_spec(&spec);
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
