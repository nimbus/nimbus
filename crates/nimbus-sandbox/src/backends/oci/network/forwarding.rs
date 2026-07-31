//! Host-machine port forwarding requests for OCI machine mode.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkProviderHandle, NetworkProviderHandleError, NetworkProviderId, NetworkResourceGeneration,
};
use serde::{Deserialize, Serialize};

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

use super::{
    DEFAULT_MACHINE_FORWARDER_HOST, DEFAULT_MACHINE_FORWARDER_PATH, DEFAULT_MACHINE_FORWARDER_PORT,
    MACHINE_FORWARDER_TIMEOUT,
};

const MACHINE_FORWARDER_PROVIDER_KEY: &str = "nimbus-sandbox.gvproxy-forwarder";
const MAX_MACHINE_FORWARDER_RESPONSE_BYTES: usize = 1024 * 1024;

mod receipt;
pub(crate) use receipt::CurrentMachinePortForwardingObservation;
pub use receipt::{MachinePortForwardOutcome, MachinePortForwardReceipt};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciMachinePortForwarderConfig {
    pub host: String,
    pub port: u16,
    pub path_prefix: String,
    provider_instance: NetworkProviderHandle,
    provider_generation: NetworkResourceGeneration,
}

impl OciMachinePortForwarderConfig {
    /// Build the stable provider-scoped identity owned by the gvproxy adapter.
    ///
    /// Lifecycle coordinators mint only the opaque instance value. Keeping the
    /// registration key here prevents upper layers from duplicating or
    /// drifting the provider's stable identity.
    pub fn gvproxy_provider_handle(
        provider_instance: impl Into<String>,
    ) -> std::result::Result<NetworkProviderHandle, NetworkProviderHandleError> {
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key(MACHINE_FORWARDER_PROVIDER_KEY),
            provider_instance,
        )
    }

    /// Configure the built-in gvproxy endpoint under lifecycle-issued authority.
    ///
    /// The provider instance and generation must come from the owner that
    /// launches and supervises gvproxy. Constructing a backend is not authority
    /// to mint a new provider identity for the fixed shared endpoint.
    pub fn gvproxy_for_provider_instance(
        provider_instance: impl Into<String>,
        provider_generation: NetworkResourceGeneration,
    ) -> std::result::Result<Self, NetworkProviderHandleError> {
        Self::for_provider_instance(
            DEFAULT_MACHINE_FORWARDER_HOST,
            DEFAULT_MACHINE_FORWARDER_PORT,
            DEFAULT_MACHINE_FORWARDER_PATH,
            provider_instance,
            provider_generation,
        )
    }

    pub fn for_provider_instance(
        host: impl Into<String>,
        port: u16,
        path_prefix: impl Into<String>,
        provider_instance: impl Into<String>,
        provider_generation: NetworkResourceGeneration,
    ) -> std::result::Result<Self, NetworkProviderHandleError> {
        Ok(Self {
            host: host.into(),
            port,
            path_prefix: path_prefix.into(),
            provider_instance: Self::gvproxy_provider_handle(provider_instance)?,
            provider_generation,
        })
    }

    pub fn provider_instance(&self) -> &NetworkProviderHandle {
        &self.provider_instance
    }

    pub fn provider_generation(&self) -> NetworkResourceGeneration {
        self.provider_generation
    }
}

pub(crate) fn expose_machine_ports(
    config: &OciMachinePortForwarderConfig,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    port_bindings: &[SandboxPortBinding],
) -> Result<Vec<MachinePortForwardReceipt>> {
    let mut attempts = Vec::with_capacity(port_bindings.len());
    for binding in port_bindings {
        let deadline = Instant::now() + MACHINE_FORWARDER_TIMEOUT;
        let request = encode_gvproxy_request(&gvproxy_route(binding), binding, "expose")?;
        attempts.push(send_machine_forwarder_request(
            config, "POST", "/expose", &request, deadline,
        ));
    }
    let current = inspect_machine_ports(config, tenant_id, sandbox_id, port_bindings);
    match current {
        Ok(current) => Ok(current.receipts().to_vec()),
        Err(inspection_error) => Err(mutation_observation_error(
            config,
            "expose",
            port_bindings,
            &attempts,
            inspection_error,
        )),
    }
}

/// Inspect the complete desired forwarding batch without mutating gvproxy.
///
/// The built-in adapter translates gvproxy's native batch `GET /all` response
/// into Nimbus-owned evidence under the exact parent-issued provider handle
/// and generation. `/expose` is never used as an inspection fallback. An
/// unavailable, unsupported, partial, duplicate, or malformed response leaves
/// current forwarding unknown.
pub(crate) fn inspect_machine_ports(
    config: &OciMachinePortForwarderConfig,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    port_bindings: &[SandboxPortBinding],
) -> Result<CurrentMachinePortForwardingObservation> {
    if port_bindings.is_empty() {
        return Ok(CurrentMachinePortForwardingObservation::authenticated(
            &config.provider_instance,
            config.provider_generation,
            Vec::new(),
        ));
    }

    let deadline = Instant::now() + MACHINE_FORWARDER_TIMEOUT;
    let response = send_machine_forwarder_request(config, "GET", "/all", &[], deadline)
        .map_err(|error| current_observation_error(config, error.to_string()))?;
    if response.status_code != 200 {
        return Err(current_observation_error(
            config,
            format!("gvproxy returned HTTP {}", response.status_code),
        ));
    }
    let routes =
        serde_json::from_slice::<Vec<GvproxyForwardRoute>>(&response.body).map_err(|error| {
            current_observation_error(
                config,
                format!("gvproxy returned a malformed forwarding list: {error}"),
            )
        })?;
    let receipts =
        authenticate_current_routes(config, tenant_id, sandbox_id, port_bindings, &routes)?;
    Ok(CurrentMachinePortForwardingObservation::authenticated(
        &config.provider_instance,
        config.provider_generation,
        receipts,
    ))
}

pub(crate) fn unexpose_machine_ports(
    config: &OciMachinePortForwarderConfig,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    port_bindings: &[SandboxPortBinding],
) -> Result<Vec<MachinePortForwardReceipt>> {
    let mut attempts = Vec::with_capacity(port_bindings.len());
    for binding in port_bindings {
        let deadline = Instant::now() + MACHINE_FORWARDER_TIMEOUT;
        let request = encode_gvproxy_request(
            &GvproxyUnexposeRequest {
                local: machine_forward_local(binding),
                protocol: "tcp".to_owned(),
            },
            binding,
            "unexpose",
        )?;
        attempts.push(send_machine_forwarder_request(
            config,
            "POST",
            "/unexpose",
            &request,
            deadline,
        ));
    }
    if port_bindings.is_empty() {
        return Ok(Vec::new());
    }
    let deadline = Instant::now() + MACHINE_FORWARDER_TIMEOUT;
    let response = send_machine_forwarder_request(config, "GET", "/all", &[], deadline);
    let routes = match response {
        Ok(response) if response.status_code == 200 => {
            serde_json::from_slice::<Vec<GvproxyForwardRoute>>(&response.body).map_err(|error| {
                mutation_observation_error(
                    config,
                    "unexpose",
                    port_bindings,
                    &attempts,
                    current_observation_error(
                        config,
                        format!("gvproxy returned a malformed forwarding list: {error}"),
                    ),
                )
            })?
        }
        Ok(response) => {
            return Err(mutation_observation_error(
                config,
                "unexpose",
                port_bindings,
                &attempts,
                current_observation_error(
                    config,
                    format!("gvproxy returned HTTP {}", response.status_code),
                ),
            ));
        }
        Err(error) => {
            return Err(mutation_observation_error(
                config,
                "unexpose",
                port_bindings,
                &attempts,
                error,
            ));
        }
    };
    let mut receipts = Vec::with_capacity(port_bindings.len());
    for (index, binding) in port_bindings.iter().enumerate() {
        if routes.iter().any(|route| route.occupies(binding)) {
            return Err(mutation_observation_error(
                config,
                "unexpose",
                port_bindings,
                &attempts,
                current_observation_error(
                    config,
                    format!(
                        "gvproxy still lists publication {}:{}",
                        binding.host_address, binding.host_port
                    ),
                ),
            ));
        }
        let outcome = if attempts
            .get(index)
            .is_some_and(|attempt| matches!(attempt, Ok(response) if response.status_code == 200))
        {
            MachinePortForwardOutcome::Withdrawn
        } else {
            MachinePortForwardOutcome::ExactAlreadyAbsent
        };
        receipts.push(MachinePortForwardReceipt::authenticated(
            outcome,
            tenant_id,
            sandbox_id,
            binding,
            &config.provider_instance,
            config.provider_generation,
        ));
    }
    Ok(receipts)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GvproxyForwardRoute {
    local: String,
    remote: String,
    protocol: String,
}

impl GvproxyForwardRoute {
    fn occupies(&self, binding: &SandboxPortBinding) -> bool {
        self.local == machine_forward_local(binding) && self.protocol == "tcp"
    }
}

#[derive(Debug, Serialize)]
struct GvproxyUnexposeRequest {
    local: String,
    protocol: String,
}

struct MachineForwarderHttpResponse {
    status_code: u16,
    body: Vec<u8>,
}

fn gvproxy_route(binding: &SandboxPortBinding) -> GvproxyForwardRoute {
    GvproxyForwardRoute {
        local: machine_forward_local(binding),
        remote: machine_forward_remote(binding),
        protocol: "tcp".to_owned(),
    }
}

fn encode_gvproxy_request(
    request: &impl Serialize,
    binding: &SandboxPortBinding,
    action: &str,
) -> Result<Vec<u8>> {
    serde_json::to_vec(request).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to encode gvproxy {action} request for {}:{}: {error}",
            binding.host_address, binding.host_port
        ),
    })
}

fn authenticate_current_routes(
    config: &OciMachinePortForwarderConfig,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    bindings: &[SandboxPortBinding],
    routes: &[GvproxyForwardRoute],
) -> Result<Vec<MachinePortForwardReceipt>> {
    let mut receipts = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let expected = gvproxy_route(binding);
        let slot_routes = routes
            .iter()
            .filter(|route| route.occupies(binding))
            .collect::<Vec<_>>();
        if slot_routes.as_slice() != [&expected] {
            return Err(current_observation_error(
                config,
                format!(
                    "gvproxy does not list exactly one expected route for {}:{}",
                    binding.host_address, binding.host_port
                ),
            ));
        }
        receipts.push(MachinePortForwardReceipt::authenticated(
            MachinePortForwardOutcome::Exposed,
            tenant_id,
            sandbox_id,
            binding,
            &config.provider_instance,
            config.provider_generation,
        ));
    }
    Ok(receipts)
}

fn machine_forward_local(binding: &SandboxPortBinding) -> String {
    format!("{}:{}", binding.host_address, binding.host_port)
}

fn current_observation_error(
    config: &OciMachinePortForwarderConfig,
    detail: impl std::fmt::Display,
) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "machine forwarder current observation is ambiguous at provider generation {}: \
             {detail}",
            config.provider_generation.as_u64(),
        ),
    }
}

fn mutation_observation_error(
    config: &OciMachinePortForwarderConfig,
    action: &str,
    bindings: &[SandboxPortBinding],
    attempts: &[Result<MachineForwarderHttpResponse>],
    inspection_error: SandboxError,
) -> SandboxError {
    let accepted = attempts
        .iter()
        .filter(|attempt| matches!(attempt, Ok(response) if response.status_code == 200))
        .count();
    SandboxError::OperationFailed {
        message: format!(
            "machine forwarder {action} batch is ambiguous at provider generation {}: \
             {accepted}/{} native mutation responses succeeded and exact current observation \
             failed: {inspection_error}",
            config.provider_generation.as_u64(),
            bindings.len(),
        ),
    }
}

fn send_machine_forwarder_request(
    config: &OciMachinePortForwarderConfig,
    method: &str,
    path_suffix: &str,
    body: &[u8],
    deadline: Instant,
) -> Result<MachineForwarderHttpResponse> {
    let remaining = remaining_before(deadline)?;
    let mut addresses = (config.host.as_str(), config.port)
        .to_socket_addrs()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to resolve machine forwarder {}:{}: {error}",
                config.host, config.port
            ),
        })?;
    let address = addresses
        .next()
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "machine forwarder {}:{} did not resolve to an address",
                config.host, config.port
            ),
        })?;
    let mut stream = TcpStream::connect_timeout(&address, remaining).map_err(|error| {
        SandboxError::OperationFailed {
            message: format!(
                "failed to connect to machine forwarder {}:{}: {error}",
                config.host, config.port
            ),
        }
    })?;
    let io_timeout = remaining_before(deadline)?;
    stream
        .set_read_timeout(Some(io_timeout))
        .and_then(|()| stream.set_write_timeout(Some(io_timeout)))
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to configure machine forwarder timeout {}:{}: {error}",
                config.host, config.port
            ),
        })?;
    let request = format!(
        "{method} {}{path_suffix} HTTP/1.0\r\nHost: {}\r\nAccept: application/json\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        trim_trailing_slash(&config.path_prefix),
        config.host,
        body.len(),
    );
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.write_all(body))
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to send machine forwarder {method} request to {}:{}: {error}",
                config.host, config.port
            ),
        })?;

    read_machine_forwarder_response(&mut stream, deadline)
}

fn read_machine_forwarder_response(
    stream: &mut TcpStream,
    deadline: Instant,
) -> Result<MachineForwarderHttpResponse> {
    let mut response = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut reached_eof = false;
    loop {
        stream
            .set_read_timeout(Some(remaining_before(deadline)?))
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to refresh machine forwarder read timeout: {error}"),
            })?;
        match stream.read(&mut chunk) {
            Ok(0) => {
                reached_eof = true;
                break;
            }
            Ok(read) => {
                if response.len().saturating_add(read) > MAX_MACHINE_FORWARDER_RESPONSE_BYTES {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "machine forwarder response exceeds {} bytes",
                            MAX_MACHINE_FORWARDER_RESPONSE_BYTES
                        ),
                    });
                }
                response.extend_from_slice(&chunk[..read]);
                if response_has_complete_declared_body(&response)? {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err(SandboxError::OperationFailed {
                    message: "machine forwarder response timed out before exact completion"
                        .to_owned(),
                });
            }
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!("failed to read machine forwarder response: {error}"),
                });
            }
        }
    }
    parse_machine_forwarder_response(response, reached_eof)
}

fn response_has_complete_declared_body(response: &[u8]) -> Result<bool> {
    let Some(header_end) = find_bytes(response, b"\r\n\r\n") else {
        return Ok(false);
    };
    let headers = std::str::from_utf8(&response[..header_end]).map_err(|_| {
        SandboxError::OperationFailed {
            message: "machine forwarder response headers are not UTF-8".to_owned(),
        }
    })?;
    let Some(content_length) = parse_content_length(headers)? else {
        return Ok(false);
    };
    Ok(response.len().saturating_sub(header_end + 4) >= content_length)
}

fn parse_machine_forwarder_response(
    response: Vec<u8>,
    reached_eof: bool,
) -> Result<MachineForwarderHttpResponse> {
    let header_end =
        find_bytes(&response, b"\r\n\r\n").ok_or_else(|| SandboxError::OperationFailed {
            message: "machine forwarder response ended before complete HTTP headers".to_owned(),
        })?;
    let headers = std::str::from_utf8(&response[..header_end]).map_err(|_| {
        SandboxError::OperationFailed {
            message: "machine forwarder response headers are not UTF-8".to_owned(),
        }
    })?;
    let status_code = headers
        .lines()
        .next()
        .and_then(|status| status.split_ascii_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| SandboxError::OperationFailed {
            message: "machine forwarder response has no valid HTTP status".to_owned(),
        })?;
    let body = &response[header_end + 4..];
    let body = match parse_content_length(headers)? {
        Some(content_length) if body.len() == content_length => body.to_vec(),
        Some(content_length) => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine forwarder response body length {} does not match declared \
                     Content-Length {content_length}",
                    body.len()
                ),
            });
        }
        None if reached_eof => body.to_vec(),
        None => {
            return Err(SandboxError::OperationFailed {
                message: "machine forwarder response lacks Content-Length and a confirmed EOF"
                    .to_owned(),
            });
        }
    };
    Ok(MachineForwarderHttpResponse { status_code, body })
}

fn parse_content_length(headers: &str) -> Result<Option<usize>> {
    let values = headers
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then_some(value.trim())
        })
        .collect::<Vec<_>>();
    match values.as_slice() {
        [] => Ok(None),
        [value] => value
            .parse::<usize>()
            .map(Some)
            .map_err(|_| SandboxError::OperationFailed {
                message: "machine forwarder response has an invalid Content-Length".to_owned(),
            }),
        _ => Err(SandboxError::OperationFailed {
            message: "machine forwarder response has duplicate Content-Length headers".to_owned(),
        }),
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn remaining_before(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| SandboxError::OperationFailed {
            message: "machine forwarder request exhausted its shared deadline".to_owned(),
        })
}

pub(super) fn machine_forward_remote(binding: &SandboxPortBinding) -> String {
    format!(":{}", binding.host_port)
}

fn trim_trailing_slash(path_prefix: &str) -> &str {
    path_prefix.trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{Ipv4Addr, Shutdown, TcpListener};
    use std::thread;
    use std::time::Duration;

    use nimbus_core::TenantId;
    use nimbus_network::NetworkResourceGeneration;

    use super::{
        MAX_MACHINE_FORWARDER_RESPONSE_BYTES, MachinePortForwardOutcome, MachinePortForwardReceipt,
        OciMachinePortForwarderConfig, expose_machine_ports as expose_machine_ports_with_identity,
        inspect_machine_ports as inspect_machine_ports_with_identity,
        unexpose_machine_ports as unexpose_machine_ports_with_identity,
    };
    use crate::instance::SandboxId;
    use crate::spec::SandboxPortBinding;

    enum ScriptedResponse {
        Bytes(Vec<u8>),
        BytesAllowDisconnect(Vec<u8>),
        Eof,
        Delay(Duration),
    }

    fn config_for(
        listener: &TcpListener,
        identity: &str,
        generation: u64,
    ) -> OciMachinePortForwarderConfig {
        OciMachinePortForwarderConfig::for_provider_instance(
            Ipv4Addr::LOCALHOST.to_string(),
            listener
                .local_addr()
                .expect("test forwarder address should resolve")
                .port(),
            "/services/forwarder",
            identity,
            NetworkResourceGeneration::new(generation),
        )
        .expect("test provider instance should validate")
    }

    fn binding() -> SandboxPortBinding {
        SandboxPortBinding::tcp("http", 18080, 8080)
    }

    fn tenant_id() -> TenantId {
        TenantId::new("tenant-forwarding-test").expect("test tenant should validate")
    }

    fn sandbox_id() -> SandboxId {
        SandboxId::new("machine-api:test-forwarding-plan")
    }

    fn expose_machine_ports(
        config: &OciMachinePortForwarderConfig,
        bindings: &[SandboxPortBinding],
    ) -> crate::error::Result<Vec<MachinePortForwardReceipt>> {
        expose_machine_ports_with_identity(config, &tenant_id(), &sandbox_id(), bindings)
    }

    fn unexpose_machine_ports(
        config: &OciMachinePortForwarderConfig,
        bindings: &[SandboxPortBinding],
    ) -> crate::error::Result<Vec<MachinePortForwardReceipt>> {
        unexpose_machine_ports_with_identity(config, &tenant_id(), &sandbox_id(), bindings)
    }

    fn inspect_machine_ports(
        config: &OciMachinePortForwarderConfig,
        bindings: &[SandboxPortBinding],
    ) -> crate::error::Result<super::CurrentMachinePortForwardingObservation> {
        inspect_machine_ports_with_identity(config, &tenant_id(), &sandbox_id(), bindings)
    }

    fn http_response(status: &str, body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.0 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn spawn_scripted_forwarder(
        listener: TcpListener,
        responses: Vec<ScriptedResponse>,
    ) -> thread::JoinHandle<Vec<String>> {
        thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().expect("request should arrive");
                requests.push(read_complete_request(&mut stream));
                match response {
                    ScriptedResponse::Bytes(response) => {
                        stream
                            .write_all(&response)
                            .expect("scripted response should write");
                        stream
                            .shutdown(Shutdown::Write)
                            .expect("response EOF should be explicit");
                        let mut trailing = [0_u8; 64];
                        while stream
                            .read(&mut trailing)
                            .expect("client shutdown should be readable")
                            != 0
                        {}
                    }
                    ScriptedResponse::BytesAllowDisconnect(response) => {
                        let _ = stream.write_all(&response);
                        let _ = stream.shutdown(Shutdown::Write);
                    }
                    ScriptedResponse::Eof => {
                        stream
                            .shutdown(Shutdown::Write)
                            .expect("empty response EOF should be explicit");
                    }
                    ScriptedResponse::Delay(delay) => thread::sleep(delay),
                }
            }
            requests
        })
    }

    fn read_complete_request(stream: &mut std::net::TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("test request timeout should configure");
        let mut request = Vec::new();
        let mut expected_len = None;
        let mut chunk = [0_u8; 1024];
        loop {
            let read = stream
                .read(&mut chunk)
                .expect("request bytes should be readable");
            assert_ne!(read, 0, "request must not close before its complete body");
            request.extend_from_slice(&chunk[..read]);
            if expected_len.is_none()
                && let Some(header_end) =
                    request.windows(4).position(|window| window == b"\r\n\r\n")
            {
                let headers = std::str::from_utf8(&request[..header_end])
                    .expect("test request headers should be UTF-8");
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then_some(value.trim())
                    })
                    .map(|value| {
                        value
                            .parse::<usize>()
                            .expect("test request length should parse")
                    })
                    .unwrap_or(0);
                expected_len = Some(header_end + 4 + content_length);
            }
            if expected_len.is_some_and(|expected| request.len() >= expected) {
                return String::from_utf8(request).expect("test request should be UTF-8");
            }
        }
    }

    fn assert_ambiguous(error: crate::error::SandboxError) {
        assert!(
            error.to_string().contains("ambiguous"),
            "the rejection must preserve the provider effect as ambiguous: {error}"
        );
    }

    fn native_routes(bindings: &[SandboxPortBinding]) -> Vec<u8> {
        serde_json::to_vec(
            &bindings
                .iter()
                .map(|binding| {
                    serde_json::json!({
                        "local": format!("{}:{}", binding.host_address, binding.host_port),
                        "remote": format!(":{}", binding.host_port),
                        "protocol": "tcp",
                    })
                })
                .collect::<Vec<_>>(),
        )
        .expect("native route list should encode")
    }

    #[test]
    fn expose_and_unexpose_translate_the_native_protocol_into_fenced_receipts() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, "test-native-mutations", 41);
        let server = spawn_scripted_forwarder(
            listener,
            vec![
                ScriptedResponse::Bytes(http_response("200 OK", &[])),
                ScriptedResponse::Bytes(http_response("200 OK", &native_routes(&[binding()]))),
                ScriptedResponse::Bytes(http_response("200 OK", &[])),
                ScriptedResponse::Bytes(http_response("200 OK", b"[]")),
            ],
        );

        let exposed = expose_machine_ports(&config, &[binding()])
            .expect("native expose plus exact list should authenticate");
        let withdrawn = unexpose_machine_ports(&config, &[binding()])
            .expect("native unexpose plus exact absence should authenticate");
        let requests = server.join().expect("test forwarder should join");

        assert_eq!(
            exposed,
            vec![MachinePortForwardReceipt {
                outcome: MachinePortForwardOutcome::Exposed,
                tenant_id: tenant_id(),
                sandbox_id: sandbox_id(),
                binding: binding(),
                provider_instance: config.provider_instance().clone(),
                provider_generation: config.provider_generation(),
            }]
        );
        assert_eq!(withdrawn[0].outcome, MachinePortForwardOutcome::Withdrawn);
        assert_eq!(requests.len(), 4);
        assert!(requests[0].starts_with("POST /services/forwarder/expose "));
        assert!(requests[1].starts_with("GET /services/forwarder/all "));
        assert!(requests[2].starts_with("POST /services/forwarder/unexpose "));
        assert!(requests[3].starts_with("GET /services/forwarder/all "));
        for (index, request) in [requests[0].as_str(), requests[2].as_str()]
            .into_iter()
            .enumerate()
        {
            let (_, body) = request
                .split_once("\r\n\r\n")
                .expect("native mutation should contain a body");
            let body: serde_json::Value =
                serde_json::from_str(body).expect("native mutation body should decode");
            assert_eq!(body["local"], "127.0.0.1:18080");
            assert_eq!(body["protocol"], "tcp");
            assert!(
                body.get("provider_instance").is_none()
                    && body.get("provider_generation").is_none(),
                "adapter-only fencing fields must not be sent to gvproxy"
            );
            if index == 0 {
                assert_eq!(body["remote"], ":18080");
            } else {
                assert!(body.get("remote").is_none());
            }
        }
    }

    #[test]
    fn current_inspection_uses_the_gvproxy_native_batch_list_contract() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, "test-native-current-inspection", 43);
        let server = spawn_scripted_forwarder(
            listener,
            vec![ScriptedResponse::Bytes(http_response(
                "200 OK",
                &native_routes(&[binding()]),
            ))],
        );

        let observation = inspect_machine_ports(&config, &[binding()])
            .expect("the exact native gvproxy route list should authenticate");
        let requests = server.join().expect("test forwarder should join");

        assert_eq!(observation.provider_instance(), config.provider_instance());
        assert_eq!(
            observation.provider_generation(),
            config.provider_generation()
        );
        assert_eq!(observation.receipts().len(), 1);
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0].starts_with("GET /services/forwarder/all HTTP/1.0\r\n"),
            "current observation must use gvproxy's one supported read-only batch route: \
             {requests:?}"
        );
        assert!(
            !requests[0].contains("/expose ") && !requests[0].contains("/inspect "),
            "current observation must neither mutate nor invent a provider route: {requests:?}"
        );
    }

    #[test]
    fn unavailable_or_wrong_current_list_never_replays_expose() {
        for (label, response) in [
            (
                "unsupported",
                http_response("404 Not Found", b"unsupported"),
            ),
            (
                "wrong-route",
                http_response(
                    "200 OK",
                    &serde_json::to_vec(&vec![serde_json::json!({
                        "local": "127.0.0.1:18081",
                        "remote": ":18081",
                        "protocol": "tcp",
                    })])
                    .expect("wrong route should encode"),
                ),
            ),
        ] {
            let listener =
                TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
            let config = config_for(&listener, label, 42);
            let server =
                spawn_scripted_forwarder(listener, vec![ScriptedResponse::Bytes(response)]);

            let error = inspect_machine_ports(&config, &[binding()])
                .expect_err("unsupported or stale inspection must remain provider-unknown");
            let requests = server.join().expect("test forwarder should join");

            assert_ambiguous(error);
            assert_eq!(requests.len(), 1);
            assert!(
                requests[0].contains("/all") && !requests[0].contains("/expose "),
                "inspection failure must not invoke a mutating fallback: {requests:?}"
            );
        }
    }

    #[test]
    fn current_inspection_substitution_matrix_returns_no_observation_or_mutating_fallback() {
        for label in [
            "generic-success",
            "unsupported",
            "missing",
            "wrong-local",
            "wrong-remote",
            "wrong-protocol",
            "duplicate",
            "conflicting-slot",
            "malformed",
            "eof",
            "oversized",
        ] {
            let listener =
                TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
            let config = config_for(&listener, label, 62);
            let exact = serde_json::json!({
                "local": "127.0.0.1:18080",
                "remote": ":18080",
                "protocol": "tcp",
            });
            let response = match label {
                "generic-success" => ScriptedResponse::Bytes(http_response("200 OK", b"{}")),
                "unsupported" => {
                    ScriptedResponse::Bytes(http_response("404 Not Found", b"unsupported"))
                }
                "missing" => ScriptedResponse::Bytes(http_response("200 OK", b"[]")),
                "wrong-local" => {
                    let mut value = exact.clone();
                    value["local"] = serde_json::json!("127.0.0.1:18081");
                    ScriptedResponse::Bytes(http_response(
                        "200 OK",
                        &serde_json::to_vec(&vec![value]).expect("response should encode"),
                    ))
                }
                "wrong-remote" => {
                    let mut value = exact.clone();
                    value["remote"] = serde_json::json!(":18081");
                    ScriptedResponse::Bytes(http_response(
                        "200 OK",
                        &serde_json::to_vec(&vec![value]).expect("response should encode"),
                    ))
                }
                "wrong-protocol" => {
                    let mut value = exact.clone();
                    value["protocol"] = serde_json::json!("udp");
                    ScriptedResponse::Bytes(http_response(
                        "200 OK",
                        &serde_json::to_vec(&vec![value]).expect("response should encode"),
                    ))
                }
                "duplicate" => ScriptedResponse::Bytes(http_response(
                    "200 OK",
                    &serde_json::to_vec(&vec![exact.clone(), exact.clone()])
                        .expect("response should encode"),
                )),
                "conflicting-slot" => {
                    let mut conflicting = exact.clone();
                    conflicting["remote"] = serde_json::json!(":18081");
                    ScriptedResponse::Bytes(http_response(
                        "200 OK",
                        &serde_json::to_vec(&vec![exact.clone(), conflicting])
                            .expect("response should encode"),
                    ))
                }
                "malformed" => ScriptedResponse::Bytes(http_response("200 OK", br#"[{"local":"#)),
                "eof" => ScriptedResponse::Eof,
                "oversized" => ScriptedResponse::BytesAllowDisconnect(http_response(
                    "200 OK",
                    &vec![b'x'; MAX_MACHINE_FORWARDER_RESPONSE_BYTES + 1],
                )),
                _ => unreachable!("the substitution labels are closed above"),
            };
            let server = spawn_scripted_forwarder(listener, vec![response]);

            let error = inspect_machine_ports(&config, &[binding()])
                .expect_err("substituted current evidence must return no observation");
            let requests = server.join().expect("test forwarder should join");

            assert_ambiguous(error);
            assert_eq!(requests.len(), 1);
            assert!(
                requests[0].contains("/all") && !requests[0].contains("/expose "),
                "{label}: current inspection must have no mutating fallback: {requests:?}"
            );
        }
    }

    #[test]
    fn current_inspection_partial_timeout_and_refusal_remain_provider_unknown() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("partial forwarder should bind");
        let config = config_for(&listener, "partial-current-inspection", 63);
        let second_binding = SandboxPortBinding::tcp("metrics", 19090, 9090);
        let server = spawn_scripted_forwarder(
            listener,
            vec![ScriptedResponse::Bytes(http_response(
                "200 OK",
                &native_routes(&[binding()]),
            ))],
        );

        let partial_error = inspect_machine_ports(&config, &[binding(), second_binding])
            .expect_err("a partial current batch must return no observation");
        let partial_requests = server.join().expect("partial forwarder should join");
        assert_ambiguous(partial_error);
        assert_eq!(partial_requests.len(), 1);
        assert!(
            partial_requests
                .iter()
                .all(|request| request.contains("/all") && !request.contains("/expose "))
        );

        let timeout_listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("timeout forwarder should bind");
        let timeout_config = config_for(&timeout_listener, "timeout-current-inspection", 64);
        let timeout_server = spawn_scripted_forwarder(
            timeout_listener,
            vec![ScriptedResponse::Delay(Duration::from_millis(2_100))],
        );
        let timeout_error = inspect_machine_ports(&timeout_config, &[binding()])
            .expect_err("a provider timeout must remain unknown");
        let timeout_requests = timeout_server
            .join()
            .expect("timeout forwarder should join");
        assert_ambiguous(timeout_error);
        assert_eq!(timeout_requests.len(), 1);
        assert!(timeout_requests[0].contains("/all") && !timeout_requests[0].contains("/expose "));

        let refusal_listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("refusal port should bind");
        let refusal_config = config_for(&refusal_listener, "refused-current-inspection", 65);
        drop(refusal_listener);
        assert_ambiguous(
            inspect_machine_ports(&refusal_config, &[binding()])
                .expect_err("connection refusal must remain unknown"),
        );
    }

    #[test]
    fn empty_current_inspection_has_no_provider_io_or_forwarding_claim() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("unused provider should bind");
        let config = config_for(&listener, "test-empty-current-inspection", 43);

        let observation =
            inspect_machine_ports(&config, &[]).expect("empty desired forwarding should inspect");

        assert_eq!(observation.provider_instance(), config.provider_instance());
        assert_eq!(
            observation.provider_generation(),
            config.provider_generation()
        );
        assert!(
            observation.receipts().is_empty(),
            "empty desired forwarding must not fabricate a route receipt"
        );
        listener
            .set_nonblocking(true)
            .expect("provider fixture should become nonblocking");
        assert!(
            matches!(
                listener.accept(),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
            ),
            "empty desired forwarding must perform no provider I/O"
        );
    }

    #[test]
    fn partial_native_observation_returns_no_mutation_success_evidence() {
        let second_binding = SandboxPortBinding::tcp("metrics", 19090, 9090);
        let bindings = vec![binding(), second_binding.clone()];

        for action in ["expose", "unexpose"] {
            let listener =
                TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
            let config = config_for(&listener, action, 51);
            let observed = if action == "expose" {
                native_routes(&[binding()])
            } else {
                native_routes(std::slice::from_ref(&second_binding))
            };
            let server = spawn_scripted_forwarder(
                listener,
                vec![
                    ScriptedResponse::Bytes(http_response("200 OK", &[])),
                    ScriptedResponse::Bytes(http_response("200 OK", &[])),
                    ScriptedResponse::Bytes(http_response("200 OK", &observed)),
                ],
            );

            let error = if action == "expose" {
                expose_machine_ports(&config, &bindings)
                    .expect_err("a partial exposed batch must return no success evidence")
            } else {
                unexpose_machine_ports(&config, &bindings)
                    .expect_err("a partial absent batch must return no success evidence")
            };
            let requests = server.join().expect("test forwarder should join");

            assert_eq!(
                requests.len(),
                3,
                "both native mutations and one complete batch observation must run"
            );
            assert_ambiguous(error);
        }
    }

    #[test]
    fn generic_http_success_is_not_machine_forwarder_evidence() {
        let expose_listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let expose_config = config_for(&expose_listener, "test-generic-expose-status", 6);
        let expose_server = spawn_scripted_forwarder(
            expose_listener,
            vec![
                ScriptedResponse::Bytes(http_response("200 OK", &[])),
                ScriptedResponse::Bytes(http_response("200 OK", b"{}")),
            ],
        );
        let expose_error = expose_machine_ports(&expose_config, &[binding()])
            .expect_err("a generic mutation status without exact list cannot prove exposure");
        let expose_requests = expose_server.join().expect("test forwarder should join");
        assert!(
            expose_requests.len() == 2
                && expose_requests[0].contains("/expose")
                && expose_requests[1].contains("/all"),
            "generic expose status must be followed by exact observation: {expose_requests:?}"
        );
        assert_ambiguous(expose_error);

        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, "test-generic-status", 7);
        let server = spawn_scripted_forwarder(
            listener,
            vec![
                ScriptedResponse::Bytes(http_response("200 OK", &[])),
                ScriptedResponse::Bytes(http_response("200 OK", &native_routes(&[binding()]))),
            ],
        );

        let error = unexpose_machine_ports(&config, &[binding()])
            .expect_err("a generic status cannot replace observed provider absence");
        let requests = server.join().expect("test forwarder should join");

        assert!(
            requests.len() == 2
                && requests[0].contains("/unexpose")
                && requests[1].contains("/all"),
            "withdrawal must remain fenced while the native list still contains the route: \
             {requests:?}"
        );
        assert_ambiguous(error);
    }

    #[test]
    fn failed_unexpose_with_exact_native_absence_is_idempotently_already_absent() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, "test-exact-absence", 8);
        let server = spawn_scripted_forwarder(
            listener,
            vec![
                ScriptedResponse::Bytes(http_response("500 Internal Server Error", b"missing")),
                ScriptedResponse::Bytes(http_response("200 OK", b"[]")),
            ],
        );

        let receipts = unexpose_machine_ports(&config, &[binding()])
            .expect("exact native absence may settle an ambiguous idempotent withdrawal");
        let requests = server.join().expect("test forwarder should join");

        assert_eq!(
            receipts[0].outcome,
            MachinePortForwardOutcome::ExactAlreadyAbsent
        );
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("/unexpose") && requests[1].contains("/all"));
    }

    #[test]
    fn successful_unexpose_plus_native_absence_emits_withdrawn_receipt() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, "test-native-withdrawal", 9);
        let server = spawn_scripted_forwarder(
            listener,
            vec![
                ScriptedResponse::Bytes(http_response("200 OK", &[])),
                ScriptedResponse::Bytes(http_response("200 OK", b"[]")),
            ],
        );

        let receipts = unexpose_machine_ports(&config, &[binding()])
            .expect("native success and exact absence may authorize withdrawal");
        let requests = server.join().expect("test forwarder should join");

        assert_eq!(receipts[0].outcome, MachinePortForwardOutcome::Withdrawn);
        assert_eq!(receipts[0].provider_instance, *config.provider_instance());
        assert_eq!(
            receipts[0].provider_generation,
            config.provider_generation()
        );
        assert_eq!(requests.len(), 2);
    }

    #[test]
    fn native_route_list_cannot_substitute_configured_provider_authority() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, "test-configured-authority", 10);
        let server = spawn_scripted_forwarder(
            listener,
            vec![ScriptedResponse::Bytes(http_response(
                "200 OK",
                &native_routes(&[binding()]),
            ))],
        );

        let observation = inspect_machine_ports(&config, &[binding()])
            .expect("native route should be translated under configured lifecycle authority");
        let requests = server.join().expect("test forwarder should join");

        assert_eq!(observation.provider_instance(), config.provider_instance());
        assert_eq!(
            observation.provider_generation(),
            config.provider_generation()
        );
        assert_eq!(requests.len(), 1);
    }

    #[test]
    fn status_eof_timeout_refusal_and_arbitrary_text_are_provider_unknown() {
        let binding = binding();

        for (label, first_response) in [
            (
                "status",
                ScriptedResponse::Bytes(http_response("204 No Content", &[])),
            ),
            ("eof", ScriptedResponse::Eof),
            (
                "text",
                ScriptedResponse::Bytes(http_response("200 OK", b"withdrawn")),
            ),
        ] {
            let listener =
                TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
            let config = config_for(&listener, label, 11);
            let server = spawn_scripted_forwarder(listener, vec![first_response]);
            let error = inspect_machine_ports(&config, std::slice::from_ref(&binding))
                .expect_err("non-evidence must remain provider-unknown");
            server.join().expect("test forwarder should join");
            assert_ambiguous(error);
        }

        let timeout_listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("timeout forwarder should bind");
        let timeout_config = config_for(&timeout_listener, "timeout", 12);
        let timeout_server = spawn_scripted_forwarder(
            timeout_listener,
            vec![ScriptedResponse::Delay(Duration::from_secs(3))],
        );
        let timeout_error = inspect_machine_ports(&timeout_config, std::slice::from_ref(&binding))
            .expect_err("timeout must not authorize withdrawal");
        timeout_server
            .join()
            .expect("timeout forwarder should join");
        assert_ambiguous(timeout_error);

        let refused_listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("refusal port should bind");
        let refused_config = config_for(&refused_listener, "refused", 13);
        drop(refused_listener);
        let refused_error = inspect_machine_ports(&refused_config, std::slice::from_ref(&binding))
            .expect_err("connection refusal must not authorize withdrawal");
        assert_ambiguous(refused_error);
    }
}
