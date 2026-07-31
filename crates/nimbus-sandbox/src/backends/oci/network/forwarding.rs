//! Host-machine port forwarding requests for OCI machine mode.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
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
pub(crate) use receipt::{
    CurrentMachinePortForwardingObservation, MachinePortForwardingSlotObservation,
};
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

#[cfg(test)]
pub(crate) fn expose_machine_ports(
    config: &OciMachinePortForwarderConfig,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    port_bindings: &[SandboxPortBinding],
) -> Result<Vec<MachinePortForwardReceipt>> {
    converge_machine_ports_without_journal(
        config,
        tenant_id,
        sandbox_id,
        port_bindings,
        MachinePortForwardingAction::Expose,
    )
}

/// Inspect the complete desired forwarding batch without mutating gvproxy.
///
/// The built-in adapter translates gvproxy's native batch `GET /all` response
/// into Nimbus-owned evidence under the exact parent-issued provider handle
/// and generation. `/expose` is never used as an inspection fallback. An
/// unavailable, unsupported, truncated, or malformed response leaves current
/// forwarding unknown. A complete list classifies every desired slot as exact
/// exposed, exact absent, or conflicting.
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
    let response =
        send_machine_forwarder_request(config, "GET", "/all", &[], deadline).map_err(|error| {
            current_observation_error(config.provider_generation, error.to_string())
        })?;
    if response.status_code != 200 {
        return Err(current_observation_error(
            config.provider_generation,
            format!("gvproxy returned HTTP {}", response.status_code),
        ));
    }
    let routes =
        serde_json::from_slice::<Vec<GvproxyForwardRoute>>(&response.body).map_err(|error| {
            current_observation_error(
                config.provider_generation,
                format!("gvproxy returned a malformed forwarding list: {error}"),
            )
        })?;
    let slots = authenticate_current_routes(config, tenant_id, sandbox_id, port_bindings, &routes)?;
    Ok(CurrentMachinePortForwardingObservation::authenticated(
        &config.provider_instance,
        config.provider_generation,
        slots,
    ))
}

#[cfg(test)]
pub(crate) fn unexpose_machine_ports(
    config: &OciMachinePortForwarderConfig,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    port_bindings: &[SandboxPortBinding],
) -> Result<Vec<MachinePortForwardReceipt>> {
    converge_machine_ports_without_journal(
        config,
        tenant_id,
        sandbox_id,
        port_bindings,
        MachinePortForwardingAction::Withdraw,
    )
}

/// Small sandbox-owned effect capability consumed by the durable publication
/// coordinator. It deliberately exposes one complete inspection and one exact
/// mutation at a time; mutation returns are diagnostic, never provider truth.
pub(crate) trait MachinePortForwardingProvider {
    fn provider_instance(&self) -> &NetworkProviderHandle;
    fn provider_generation(&self) -> NetworkResourceGeneration;
    fn inspect(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
    ) -> Result<CurrentMachinePortForwardingObservation>;
    fn expose_one(&self, binding: &SandboxPortBinding) -> Result<MachinePortMutationDiagnostic>;
    fn withdraw_one(&self, binding: &SandboxPortBinding) -> Result<MachinePortMutationDiagnostic>;
}

/// Deterministic current-provider substitute used by lifecycle tests that own
/// no native gvproxy process.
#[cfg(test)]
pub(crate) struct DeterministicMachinePortForwardingProvider {
    config: OciMachinePortForwarderConfig,
    exposed: bool,
}

#[cfg(test)]
impl DeterministicMachinePortForwardingProvider {
    pub(crate) fn exposed(config: &OciMachinePortForwarderConfig) -> Self {
        Self {
            config: config.clone(),
            exposed: true,
        }
    }

    pub(crate) fn absent(config: &OciMachinePortForwarderConfig) -> Self {
        Self {
            config: config.clone(),
            exposed: false,
        }
    }
}

#[cfg(test)]
impl MachinePortForwardingProvider for DeterministicMachinePortForwardingProvider {
    fn provider_instance(&self) -> &NetworkProviderHandle {
        self.config.provider_instance()
    }

    fn provider_generation(&self) -> NetworkResourceGeneration {
        self.config.provider_generation()
    }

    fn inspect(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
    ) -> Result<CurrentMachinePortForwardingObservation> {
        let slots = bindings
            .iter()
            .map(|binding| {
                let receipt = MachinePortForwardReceipt::authenticated(
                    if self.exposed {
                        MachinePortForwardOutcome::Exposed
                    } else {
                        MachinePortForwardOutcome::ExactAlreadyAbsent
                    },
                    tenant_id,
                    sandbox_id,
                    binding,
                    self.provider_instance(),
                    self.provider_generation(),
                );
                if self.exposed {
                    MachinePortForwardingSlotObservation::Exposed(receipt)
                } else {
                    MachinePortForwardingSlotObservation::Absent(receipt)
                }
            })
            .collect();
        Ok(CurrentMachinePortForwardingObservation::authenticated(
            self.provider_instance(),
            self.provider_generation(),
            slots,
        ))
    }

    fn expose_one(&self, _binding: &SandboxPortBinding) -> Result<MachinePortMutationDiagnostic> {
        Ok(MachinePortMutationDiagnostic {
            status_accepted: true,
        })
    }

    fn withdraw_one(&self, _binding: &SandboxPortBinding) -> Result<MachinePortMutationDiagnostic> {
        Ok(MachinePortMutationDiagnostic {
            status_accepted: true,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MachinePortMutationDiagnostic {
    status_accepted: bool,
}

impl MachinePortMutationDiagnostic {
    fn from_response(response: MachineForwarderHttpResponse) -> Self {
        Self {
            status_accepted: response.status_code == 200,
        }
    }

    pub(crate) fn status_accepted(self) -> bool {
        self.status_accepted
    }

    #[cfg(test)]
    pub(crate) const fn accepted() -> Self {
        Self {
            status_accepted: true,
        }
    }
}

impl MachinePortForwardingProvider for OciMachinePortForwarderConfig {
    fn provider_instance(&self) -> &NetworkProviderHandle {
        self.provider_instance()
    }

    fn provider_generation(&self) -> NetworkResourceGeneration {
        self.provider_generation()
    }

    fn inspect(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
    ) -> Result<CurrentMachinePortForwardingObservation> {
        inspect_machine_ports(self, tenant_id, sandbox_id, bindings)
    }

    fn expose_one(&self, binding: &SandboxPortBinding) -> Result<MachinePortMutationDiagnostic> {
        let deadline = Instant::now() + MACHINE_FORWARDER_TIMEOUT;
        let request = encode_gvproxy_request(&gvproxy_route(binding), binding, "expose")?;
        send_machine_forwarder_request(self, "POST", "/expose", &request, deadline)
            .map(MachinePortMutationDiagnostic::from_response)
    }

    fn withdraw_one(&self, binding: &SandboxPortBinding) -> Result<MachinePortMutationDiagnostic> {
        let deadline = Instant::now() + MACHINE_FORWARDER_TIMEOUT;
        let request = encode_gvproxy_request(
            &GvproxyUnexposeRequest {
                local: machine_forward_local(binding),
                protocol: "tcp".to_owned(),
            },
            binding,
            "unexpose",
        )?;
        send_machine_forwarder_request(self, "POST", "/unexpose", &request, deadline)
            .map(MachinePortMutationDiagnostic::from_response)
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
enum MachinePortForwardingAction {
    Expose,
    Withdraw,
}

#[cfg(test)]
impl MachinePortForwardingAction {
    fn label(self) -> &'static str {
        match self {
            Self::Expose => "expose",
            Self::Withdraw => "unexpose",
        }
    }

    fn receipt(
        self,
        observation: &MachinePortForwardingSlotObservation,
    ) -> Option<&MachinePortForwardReceipt> {
        match self {
            Self::Expose => observation.exposed_receipt(),
            Self::Withdraw => observation.absent_receipt(),
        }
    }
}

#[cfg(test)]
fn converge_machine_ports_without_journal(
    provider: &impl MachinePortForwardingProvider,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    bindings: &[SandboxPortBinding],
    action: MachinePortForwardingAction,
) -> Result<Vec<MachinePortForwardReceipt>> {
    let mut receipts = Vec::with_capacity(bindings.len());
    for (index, binding) in bindings.iter().enumerate() {
        let before = provider.inspect(tenant_id, sandbox_id, bindings)?;
        authenticate_observation_identity(provider, &before)?;
        let slot = before.slots().get(index).ok_or_else(|| {
            current_observation_error(
                provider.provider_generation(),
                format!("provider omitted canonical slot {index}"),
            )
        })?;
        if let Some(receipt) = action.receipt(slot) {
            receipts.push(receipt.clone());
            continue;
        }
        if let Some(detail) = slot.conflict_detail() {
            return Err(current_observation_error(
                provider.provider_generation(),
                detail,
            ));
        }

        let mutation = match action {
            MachinePortForwardingAction::Expose => provider.expose_one(binding),
            MachinePortForwardingAction::Withdraw => provider.withdraw_one(binding),
        };
        let after =
            provider
                .inspect(tenant_id, sandbox_id, bindings)
                .map_err(|inspection_error| {
                    mutation_observation_error(
                        provider.provider_generation(),
                        action.label(),
                        binding,
                        mutation.as_ref().ok().copied(),
                        mutation.as_ref().err(),
                        inspection_error,
                    )
                })?;
        authenticate_observation_identity(provider, &after)?;
        let slot = after.slots().get(index).ok_or_else(|| {
            current_observation_error(
                provider.provider_generation(),
                format!("provider omitted canonical slot {index} after mutation"),
            )
        })?;
        let Some(receipt) = action.receipt(slot) else {
            let detail = slot
                .conflict_detail()
                .unwrap_or("provider still reports the pre-mutation slot state");
            return Err(mutation_observation_error(
                provider.provider_generation(),
                action.label(),
                binding,
                mutation.as_ref().ok().copied(),
                mutation.as_ref().err(),
                current_observation_error(provider.provider_generation(), detail),
            ));
        };
        let mut receipt = receipt.clone();
        if matches!(action, MachinePortForwardingAction::Withdraw)
            && mutation
                .as_ref()
                .is_ok_and(|diagnostic| diagnostic.status_accepted())
        {
            receipt.outcome = MachinePortForwardOutcome::Withdrawn;
        }
        receipts.push(receipt);
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

#[derive(Debug, Serialize)]
struct GvproxyUnexposeRequest {
    local: String,
    protocol: String,
}

struct MachineForwarderHttpResponse {
    status_code: u16,
    body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GvproxyLocalEndpoint {
    NativeWildcard(u16),
    Socket(SocketAddr),
}

impl GvproxyLocalEndpoint {
    fn parse(local: &str) -> std::result::Result<Self, String> {
        if let Some(port) = local.strip_prefix(':') {
            return port
                .parse::<u16>()
                .map(Self::NativeWildcard)
                .map_err(|error| error.to_string());
        }
        local
            .parse::<SocketAddr>()
            .map(Self::Socket)
            .map_err(|error| error.to_string())
    }

    const fn port(self) -> u16 {
        match self {
            Self::NativeWildcard(port) => port,
            Self::Socket(address) => address.port(),
        }
    }

    fn overlaps(self, desired_address: IpAddr, desired_port: u16) -> bool {
        self.port() == desired_port
            && match self {
                Self::NativeWildcard(_) => true,
                Self::Socket(actual) => {
                    let actual = canonical_ip_address(actual.ip());
                    let desired = canonical_ip_address(desired_address);
                    actual == desired || actual.is_unspecified() || desired.is_unspecified()
                }
            }
    }

    fn exactly_represents(self, desired_address: IpAddr, desired_port: u16) -> bool {
        self.port() == desired_port
            && match self {
                Self::NativeWildcard(_) => desired_address.is_unspecified(),
                Self::Socket(actual) => actual.ip() == desired_address,
            }
    }
}

fn canonical_ip_address(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V4(address) => IpAddr::V4(address),
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map_or(IpAddr::V6(address), IpAddr::V4),
    }
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
) -> Result<Vec<MachinePortForwardingSlotObservation>> {
    let parsed_routes = routes
        .iter()
        .filter_map(|route| match route.protocol.as_str() {
            "tcp" => Some(
                GvproxyLocalEndpoint::parse(&route.local)
                    .map(|local| (route, local))
                    .map_err(|error| {
                        current_observation_error(
                            config.provider_generation,
                            format!(
                                "gvproxy returned invalid TCP local endpoint {:?}: {error}",
                                route.local
                            ),
                        )
                    }),
            ),
            "udp" | "unix" | "npipe" => None,
            protocol => Some(Err(current_observation_error(
                config.provider_generation,
                format!("gvproxy returned unknown forwarding protocol {protocol:?}"),
            ))),
        })
        .collect::<Result<Vec<_>>>()?;
    let mut slots = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let expected = gvproxy_route(binding);
        let slot_routes = parsed_routes
            .iter()
            .filter(|(_, local)| local.overlaps(binding.host_address, binding.host_port))
            .copied()
            .collect::<Vec<_>>();
        match slot_routes.as_slice() {
            [] => slots.push(MachinePortForwardingSlotObservation::Absent(
                MachinePortForwardReceipt::authenticated(
                    MachinePortForwardOutcome::ExactAlreadyAbsent,
                    tenant_id,
                    sandbox_id,
                    binding,
                    &config.provider_instance,
                    config.provider_generation,
                ),
            )),
            [(route, local)]
                if local.exactly_represents(binding.host_address, binding.host_port)
                    && route.remote == expected.remote =>
            {
                slots.push(MachinePortForwardingSlotObservation::Exposed(
                    MachinePortForwardReceipt::authenticated(
                        MachinePortForwardOutcome::Exposed,
                        tenant_id,
                        sandbox_id,
                        binding,
                        &config.provider_instance,
                        config.provider_generation,
                    ),
                ));
            }
            _ => slots.push(MachinePortForwardingSlotObservation::Conflicting {
                binding: binding.clone(),
                detail: format!(
                    "gvproxy lists {} conflicting routes for {}:{}",
                    slot_routes.len(),
                    binding.host_address,
                    binding.host_port
                ),
            }),
        }
    }
    Ok(slots)
}

#[cfg(test)]
fn authenticate_observation_identity(
    provider: &impl MachinePortForwardingProvider,
    observation: &CurrentMachinePortForwardingObservation,
) -> Result<()> {
    if observation.provider_instance() != provider.provider_instance()
        || observation.provider_generation() != provider.provider_generation()
    {
        return Err(current_observation_error(
            provider.provider_generation(),
            "provider observation crossed the selected instance or generation",
        ));
    }
    Ok(())
}

fn current_observation_error(
    provider_generation: NetworkResourceGeneration,
    detail: impl std::fmt::Display,
) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "machine forwarder current observation is ambiguous at provider generation {}: \
             {detail}",
            provider_generation.as_u64(),
        ),
    }
}

#[cfg(test)]
fn mutation_observation_error(
    provider_generation: NetworkResourceGeneration,
    action: &str,
    binding: &SandboxPortBinding,
    diagnostic: Option<MachinePortMutationDiagnostic>,
    mutation_error: Option<&SandboxError>,
    inspection_error: SandboxError,
) -> SandboxError {
    let mutation_detail = match (diagnostic, mutation_error) {
        (Some(diagnostic), _) => format!("native status accepted={}", diagnostic.status_accepted()),
        (None, Some(error)) => format!("native request failed: {error}"),
        (None, None) => "native request outcome unavailable".to_owned(),
    };
    SandboxError::OperationFailed {
        message: format!(
            "machine forwarder {action} for {}:{} is ambiguous at provider generation {} \
             ({mutation_detail}); exact current observation failed: {inspection_error}",
            binding.host_address,
            binding.host_port,
            provider_generation.as_u64(),
        ),
    }
}

fn machine_forward_local(binding: &SandboxPortBinding) -> String {
    binding.host_socket_addr().to_string()
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
#[path = "forwarding/tests.rs"]
mod tests;
