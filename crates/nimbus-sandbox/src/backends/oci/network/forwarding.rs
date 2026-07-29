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

use super::dto::MachinePortForwardRequest;
use super::{
    DEFAULT_MACHINE_FORWARDER_HOST, DEFAULT_MACHINE_FORWARDER_PATH, DEFAULT_MACHINE_FORWARDER_PORT,
    MACHINE_FORWARDER_TIMEOUT,
};

const MACHINE_FORWARDER_PROVIDER_KEY: &str = "nimbus-sandbox.gvproxy-forwarder";
const MAX_MACHINE_FORWARDER_RESPONSE_BYTES: usize = 1024 * 1024;

mod receipt;
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
    let mut receipts = Vec::with_capacity(port_bindings.len());
    for binding in port_bindings {
        let deadline = Instant::now() + MACHINE_FORWARDER_TIMEOUT;
        let request = machine_port_forward_request(config, binding, true)?;
        let attempted =
            send_machine_forwarder_request(config, "POST", "/expose", &request, deadline);
        if let Ok(response) = &attempted
            && let Some(outcome) = authenticate_response(
                config,
                binding,
                Some(machine_forward_remote(binding)),
                response,
                &[MachinePortForwardOutcome::Exposed],
            )
        {
            receipts.push(MachinePortForwardReceipt::authenticated(
                outcome,
                tenant_id,
                sandbox_id,
                binding,
                &config.provider_instance,
                config.provider_generation,
            ));
            continue;
        }
        return Err(ambiguous_forwarder_error(
            config,
            binding,
            "expose",
            &attempted,
            "the response did not authenticate the exact provider instance, generation, \
             publication, and exposed outcome",
        ));
    }
    Ok(receipts)
}

pub(crate) fn unexpose_machine_ports(
    config: &OciMachinePortForwarderConfig,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    port_bindings: &[SandboxPortBinding],
) -> Result<Vec<MachinePortForwardReceipt>> {
    let mut receipts = Vec::with_capacity(port_bindings.len());
    for binding in port_bindings {
        receipts.push(withdraw_machine_port(
            config, tenant_id, sandbox_id, binding,
        )?);
    }
    Ok(receipts)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct GvproxyMachinePortForwardReceipt {
    outcome: MachinePortForwardOutcome,
    provider_instance: NetworkProviderHandle,
    provider_generation: NetworkResourceGeneration,
    local: String,
    remote: Option<String>,
    protocol: String,
}

struct MachineForwarderHttpResponse {
    status_code: u16,
    body: Vec<u8>,
}

fn withdraw_machine_port(
    config: &OciMachinePortForwarderConfig,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    binding: &SandboxPortBinding,
) -> Result<MachinePortForwardReceipt> {
    let deadline = Instant::now() + MACHINE_FORWARDER_TIMEOUT;
    let request = machine_port_forward_request(config, binding, false)?;
    let attempted = send_machine_forwarder_request(config, "POST", "/unexpose", &request, deadline);

    if let Ok(response) = &attempted
        && let Some(outcome) = authenticate_response(
            config,
            binding,
            None,
            response,
            &[
                MachinePortForwardOutcome::Withdrawn,
                MachinePortForwardOutcome::ExactAlreadyAbsent,
            ],
        )
    {
        return Ok(MachinePortForwardReceipt::authenticated(
            outcome,
            tenant_id,
            sandbox_id,
            binding,
            &config.provider_instance,
            config.provider_generation,
        ));
    }

    Err(ambiguous_forwarder_error(
        config,
        binding,
        "unexpose",
        &attempted,
        "the response did not authenticate the exact provider instance, generation, publication, \
         and outcome",
    ))
}

fn machine_port_forward_request(
    config: &OciMachinePortForwarderConfig,
    binding: &SandboxPortBinding,
    expose: bool,
) -> Result<Vec<u8>> {
    serde_json::to_vec(&MachinePortForwardRequest {
        provider_instance: config.provider_instance.clone(),
        provider_generation: config.provider_generation,
        local: machine_forward_local(binding),
        remote: expose.then(|| machine_forward_remote(binding)),
        protocol: "tcp".to_owned(),
    })
    .map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to encode machine port-forward request for {}:{}: {error}",
            binding.host_address, binding.host_port
        ),
    })
}

fn authenticate_response(
    config: &OciMachinePortForwarderConfig,
    binding: &SandboxPortBinding,
    remote: Option<String>,
    response: &MachineForwarderHttpResponse,
    allowed_outcomes: &[MachinePortForwardOutcome],
) -> Option<MachinePortForwardOutcome> {
    if response.status_code != 200 {
        return None;
    }
    let receipt =
        serde_json::from_slice::<GvproxyMachinePortForwardReceipt>(&response.body).ok()?;
    (receipt.provider_instance == config.provider_instance
        && receipt.provider_generation == config.provider_generation
        && receipt.local == machine_forward_local(binding)
        && receipt.remote == remote
        && receipt.protocol == "tcp"
        && allowed_outcomes.contains(&receipt.outcome))
    .then_some(receipt.outcome)
}

fn machine_forward_local(binding: &SandboxPortBinding) -> String {
    format!("{}:{}", binding.host_address, binding.host_port)
}

fn ambiguous_forwarder_error(
    config: &OciMachinePortForwarderConfig,
    binding: &SandboxPortBinding,
    action: &str,
    attempted: &Result<MachineForwarderHttpResponse>,
    inspection: &str,
) -> SandboxError {
    let attempt = match attempted {
        Ok(response) => format!("untyped HTTP {}", response.status_code),
        Err(error) => error.to_string(),
    };
    SandboxError::OperationFailed {
        message: format!(
            "machine forwarder {action} for {}:{} is ambiguous at provider generation {}: \
             {attempt}; {inspection}",
            binding.host_address,
            binding.host_port,
            config.provider_generation.as_u64(),
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
        MachinePortForwardOutcome, MachinePortForwardReceipt, OciMachinePortForwarderConfig,
        expose_machine_ports as expose_machine_ports_with_identity,
        unexpose_machine_ports as unexpose_machine_ports_with_identity,
    };
    use crate::instance::SandboxId;
    use crate::spec::SandboxPortBinding;

    enum ScriptedResponse {
        Bytes(Vec<u8>),
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

    #[test]
    fn expose_and_unexpose_requests_carry_exact_provider_context() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, "test-request-context", 41);
        let exposed = serde_json::to_vec(&serde_json::json!({
            "outcome": "exposed",
            "provider_instance": config.provider_instance(),
            "provider_generation": config.provider_generation(),
            "local": "127.0.0.1:18080",
            "remote": ":18080",
            "protocol": "tcp",
        }))
        .expect("typed expose receipt should encode");
        let withdrawn = serde_json::to_vec(&serde_json::json!({
            "outcome": "withdrawn",
            "provider_instance": config.provider_instance(),
            "provider_generation": config.provider_generation(),
            "local": "127.0.0.1:18080",
            "remote": null,
            "protocol": "tcp",
        }))
        .expect("typed unexpose receipt should encode");
        let server = spawn_scripted_forwarder(
            listener,
            vec![
                ScriptedResponse::Bytes(http_response("200 OK", &exposed)),
                ScriptedResponse::Bytes(http_response("200 OK", &withdrawn)),
            ],
        );

        let exposed_receipts = expose_machine_ports(&config, &[binding()])
            .expect("exact typed expose receipt should authenticate");
        let absent_receipts = unexpose_machine_ports(&config, &[binding()])
            .expect("exact typed unexpose receipt should authenticate");
        let requests = server.join().expect("test forwarder should join");

        assert_eq!(
            exposed_receipts,
            vec![MachinePortForwardReceipt {
                outcome: MachinePortForwardOutcome::Exposed,
                tenant_id: tenant_id(),
                sandbox_id: sandbox_id(),
                binding: binding(),
                provider_instance: config.provider_instance().clone(),
                provider_generation: config.provider_generation(),
            }]
        );
        assert_eq!(
            absent_receipts[0].outcome,
            MachinePortForwardOutcome::Withdrawn
        );
        assert_eq!(absent_receipts[0].binding, binding());
        assert_eq!(requests.len(), 2);
        for (index, request) in requests.iter().enumerate() {
            let (_, body) = request
                .split_once("\r\n\r\n")
                .expect("request should contain headers and body");
            let body: serde_json::Value =
                serde_json::from_str(body).expect("request body should be typed JSON");
            assert_eq!(
                body["provider_instance"],
                serde_json::to_value(config.provider_instance())
                    .expect("provider instance should serialize")
            );
            assert_eq!(
                body["provider_generation"],
                serde_json::to_value(config.provider_generation())
                    .expect("provider generation should serialize")
            );
            assert_eq!(body["local"], "127.0.0.1:18080");
            assert_eq!(body["protocol"], "tcp");
            if index == 0 {
                assert!(request.contains("/expose"));
                assert_eq!(body["remote"], ":18080");
            } else {
                assert!(request.contains("/unexpose"));
                assert!(
                    body.get("remote").is_none(),
                    "unexpose must carry the same context without inventing a remote target"
                );
            }
        }
    }

    #[test]
    fn partial_and_stale_receipt_batches_return_no_success_evidence() {
        let second_binding = SandboxPortBinding::tcp("metrics", 19090, 9090);
        let bindings = vec![binding(), second_binding.clone()];

        for action in ["expose", "unexpose"] {
            let listener =
                TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
            let config = config_for(&listener, action, 51);
            let first_outcome = if action == "expose" {
                "exposed"
            } else {
                "withdrawn"
            };
            let first_remote = if action == "expose" {
                serde_json::json!(":18080")
            } else {
                serde_json::Value::Null
            };
            let first = serde_json::to_vec(&serde_json::json!({
                "outcome": first_outcome,
                "provider_instance": config.provider_instance(),
                "provider_generation": config.provider_generation(),
                "local": "127.0.0.1:18080",
                "remote": first_remote,
                "protocol": "tcp",
            }))
            .expect("first exact receipt should encode");
            let second = serde_json::to_vec(&serde_json::json!({
                "outcome": first_outcome,
                "provider_instance": config.provider_instance(),
                "provider_generation": 50,
                "local": "127.0.0.1:19090",
                "remote": if action == "expose" {
                    serde_json::json!(":19090")
                } else {
                    serde_json::Value::Null
                },
                "protocol": "tcp",
            }))
            .expect("stale second receipt should encode");
            let server = spawn_scripted_forwarder(
                listener,
                vec![
                    ScriptedResponse::Bytes(http_response("200 OK", &first)),
                    ScriptedResponse::Bytes(http_response("200 OK", &second)),
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
                2,
                "both operations must reach the stale member before rejecting the batch"
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
            vec![ScriptedResponse::Bytes(http_response("200 OK", &[]))],
        );
        let expose_error = expose_machine_ports(&expose_config, &[binding()])
            .expect_err("a generic status cannot authenticate exact provider exposure");
        let expose_requests = expose_server.join().expect("test forwarder should join");
        assert!(
            expose_requests.len() == 1 && expose_requests[0].contains("/expose"),
            "generic expose status must remain fenced: {expose_requests:?}"
        );
        assert_ambiguous(expose_error);

        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, "test-generic-status", 7);
        let server = spawn_scripted_forwarder(
            listener,
            vec![ScriptedResponse::Bytes(http_response("200 OK", &[]))],
        );

        let error = unexpose_machine_ports(&config, &[binding()])
            .expect_err("a generic status cannot authenticate exact provider withdrawal");
        let requests = server.join().expect("test forwarder should join");

        assert!(
            requests.len() == 1 && requests[0].contains("/unexpose"),
            "generic status must remain fenced without an unauthenticated inspection fallback: \
             {requests:?}"
        );
        assert_ambiguous(error);
    }

    #[test]
    fn unauthenticated_global_absence_cannot_authorize_withdrawal() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, "test-exact-absence", 8);
        let server = spawn_scripted_forwarder(
            listener,
            vec![ScriptedResponse::Bytes(http_response("200 OK", &[]))],
        );

        let error = unexpose_machine_ports(&config, &[binding()])
            .expect_err("untyped status cannot be upgraded by global address absence");
        let requests = server.join().expect("test forwarder should join");

        assert!(
            requests.len() == 1 && requests[0].contains("/unexpose"),
            "cleanup must not query unauthenticated global publication state: {requests:?}"
        );
        assert_ambiguous(error);
    }

    #[test]
    fn exact_typed_withdrawal_receipt_authenticates_instance_and_generation() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, "test-typed-withdrawal", 9);
        let body = serde_json::to_vec(&serde_json::json!({
            "outcome": "withdrawn",
            "provider_instance": config.provider_instance(),
            "provider_generation": config.provider_generation(),
            "local": "127.0.0.1:18080",
            "remote": null,
            "protocol": "tcp",
        }))
        .expect("typed receipt should encode");
        let server = spawn_scripted_forwarder(
            listener,
            vec![ScriptedResponse::Bytes(http_response("200 OK", &body))],
        );

        unexpose_machine_ports(&config, &[binding()])
            .expect("an exact typed receipt may authorize withdrawal");
        let requests = server.join().expect("test forwarder should join");

        assert_eq!(requests.len(), 1, "typed evidence needs no inference");
    }

    #[test]
    fn substituted_typed_generation_is_ambiguous_and_inspected() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, "test-stale-generation", 10);
        let body = serde_json::to_vec(&serde_json::json!({
            "outcome": "withdrawn",
            "provider_instance": config.provider_instance(),
            "provider_generation": 9,
            "local": "127.0.0.1:18080",
            "remote": null,
            "protocol": "tcp",
        }))
        .expect("stale receipt should encode");
        let server = spawn_scripted_forwarder(
            listener,
            vec![ScriptedResponse::Bytes(http_response("200 OK", &body))],
        );

        let error = unexpose_machine_ports(&config, &[binding()])
            .expect_err("a substituted provider generation must not authorize release");
        let requests = server.join().expect("test forwarder should join");

        assert_eq!(
            requests.len(),
            1,
            "stale typed evidence must remain fenced without unauthenticated fallback"
        );
        assert_ambiguous(error);
    }

    #[test]
    fn substituted_typed_provider_instance_is_ambiguous() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, "test-exact-instance", 11);
        let substituted = config_for(&listener, "test-substituted-instance", 11);
        let body = serde_json::to_vec(&serde_json::json!({
            "outcome": "withdrawn",
            "provider_instance": substituted.provider_instance(),
            "provider_generation": config.provider_generation(),
            "local": "127.0.0.1:18080",
            "remote": null,
            "protocol": "tcp",
        }))
        .expect("substituted receipt should encode");
        let server = spawn_scripted_forwarder(
            listener,
            vec![ScriptedResponse::Bytes(http_response("200 OK", &body))],
        );

        let error = unexpose_machine_ports(&config, &[binding()])
            .expect_err("a substituted provider instance must not authorize release");
        server.join().expect("test forwarder should join");
        assert_ambiguous(error);
    }

    #[test]
    fn status_eof_timeout_refusal_and_arbitrary_text_are_ambiguous() {
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
            let error = unexpose_machine_ports(&config, std::slice::from_ref(&binding))
                .expect_err("non-evidence must remain ambiguous while publication is present");
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
        let timeout_error = unexpose_machine_ports(&timeout_config, std::slice::from_ref(&binding))
            .expect_err("timeout must not authorize withdrawal");
        timeout_server
            .join()
            .expect("timeout forwarder should join");
        assert_ambiguous(timeout_error);

        let refused_listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("refusal port should bind");
        let refused_config = config_for(&refused_listener, "refused", 13);
        drop(refused_listener);
        let refused_error = unexpose_machine_ports(&refused_config, std::slice::from_ref(&binding))
            .expect_err("connection refusal must not authorize withdrawal");
        assert_ambiguous(refused_error);
    }
}
