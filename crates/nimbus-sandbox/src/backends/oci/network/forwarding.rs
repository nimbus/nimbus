//! Host-machine port forwarding requests for OCI machine mode.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::fd::{AsRawFd as _, FromRawFd as _};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt as _;
#[cfg(unix)]
use std::os::unix::net::UnixStream;

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
mod retirement;
pub(crate) use receipt::{
    CurrentMachinePortForwardingObservation, MachinePortForwardingSlotObservation,
};
pub use receipt::{MachinePortForwardOutcome, MachinePortForwardReceipt};
pub use retirement::{
    MachinePortForwardingRetirement, MachinePortForwardingRetirementObservation,
    OciMachinePortForwardingRetirement,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciMachinePortForwarderConfig {
    pub host: String,
    pub port: u16,
    pub path_prefix: String,
    unix_socket_path: Option<PathBuf>,
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
            unix_socket_path: None,
            provider_instance: Self::gvproxy_provider_handle(provider_instance)?,
            provider_generation,
        })
    }

    /// Configure a parent-reachable gvproxy services socket.
    pub fn for_unix_services_socket(
        socket_path: impl Into<PathBuf>,
        path_prefix: impl Into<String>,
        provider_instance: impl Into<String>,
        provider_generation: NetworkResourceGeneration,
    ) -> Result<Self> {
        let socket_path = socket_path.into();
        let path_prefix = path_prefix.into();
        if !socket_path.is_absolute() {
            return Err(SandboxError::InvalidSpec {
                message: "machine forwarder services socket path must be absolute".to_owned(),
            });
        }
        if path_prefix != DEFAULT_MACHINE_FORWARDER_PATH {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "machine forwarder services path must be {DEFAULT_MACHINE_FORWARDER_PATH}"
                ),
            });
        }
        #[cfg(unix)]
        {
            let path_bytes = socket_path.as_os_str().as_bytes();
            if path_bytes.contains(&0) {
                return Err(SandboxError::InvalidSpec {
                    message: "machine forwarder services socket path contains a NUL byte"
                        .to_owned(),
                });
            }
            if path_bytes.len() > unix_socket_path_max_bytes() {
                return Err(SandboxError::InvalidSpec {
                    message: format!(
                        "machine forwarder services socket path {} exceeds the platform limit of {} bytes",
                        socket_path.display(),
                        unix_socket_path_max_bytes()
                    ),
                });
            }
        }
        #[cfg(not(unix))]
        return Err(SandboxError::BackendUnavailable {
            message: "machine forwarder Unix services sockets are unavailable on this platform"
                .to_owned(),
        });

        Ok(Self {
            host: "localhost".to_owned(),
            port: 0,
            path_prefix,
            unix_socket_path: Some(socket_path),
            provider_instance: Self::gvproxy_provider_handle(provider_instance).map_err(
                |error| SandboxError::InvalidSpec {
                    message: format!("machine forwarder provider identity is invalid: {error}"),
                },
            )?,
            provider_generation,
        })
    }

    pub fn unix_socket_path(&self) -> Option<&Path> {
        self.unix_socket_path.as_deref()
    }

    pub fn provider_instance(&self) -> &NetworkProviderHandle {
        &self.provider_instance
    }

    pub fn provider_generation(&self) -> NetworkResourceGeneration {
        self.provider_generation
    }

    fn endpoint_label(&self) -> String {
        self.unix_socket_path.as_ref().map_or_else(
            || format!("{}:{}", self.host, self.port),
            |path| path.display().to_string(),
        )
    }

    /// Prove that the configured control endpoint serves the gvproxy API.
    ///
    /// Machine composition uses this before it advertises the forwarding
    /// capability. The probe performs one bounded read-only route-list request.
    pub fn require_reachable(&self) -> Result<()> {
        fetch_machine_forwarder_routes(self).map(|_| ())
    }
}

#[cfg(unix)]
const fn unix_socket_path_max_bytes() -> usize {
    std::mem::size_of::<libc::sockaddr_un>() - std::mem::offset_of!(libc::sockaddr_un, sun_path) - 1
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

    let routes = fetch_machine_forwarder_routes(config).map_err(|error| {
        current_observation_error(config.provider_generation, error.to_string())
    })?;
    let slots = authenticate_current_routes(config, tenant_id, sandbox_id, port_bindings, &routes)?;
    Ok(CurrentMachinePortForwardingObservation::authenticated(
        &config.provider_instance,
        config.provider_generation,
        slots,
    ))
}

fn fetch_machine_forwarder_routes(
    config: &OciMachinePortForwarderConfig,
) -> Result<Vec<GvproxyForwardRoute>> {
    let deadline = Instant::now() + MACHINE_FORWARDER_TIMEOUT;
    let response = send_machine_forwarder_request(config, "GET", "/all", &[], deadline)?;
    if response.status_code != 200 {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "machine forwarder {} returned HTTP {} for its services probe",
                config.endpoint_label(),
                response.status_code
            ),
        });
    }
    serde_json::from_slice(&response.body).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "machine forwarder {} returned a malformed forwarding list: {error}",
            config.endpoint_label()
        ),
    })
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

#[derive(Debug, Clone, Copy)]
enum MachinePortForwardingAction {
    #[cfg(test)]
    Expose,
    Withdraw,
}

impl MachinePortForwardingAction {
    fn label(self) -> &'static str {
        match self {
            #[cfg(test)]
            Self::Expose => "expose",
            Self::Withdraw => "unexpose",
        }
    }

    fn receipt(
        self,
        observation: &MachinePortForwardingSlotObservation,
    ) -> Option<&MachinePortForwardReceipt> {
        match self {
            #[cfg(test)]
            Self::Expose => observation.exposed_receipt(),
            Self::Withdraw => observation.absent_receipt(),
        }
    }
}

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
            #[cfg(test)]
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
    let mut stream = connect_machine_forwarder(config, remaining)?;
    let io_timeout = remaining_before(deadline)?;
    stream
        .set_read_timeout(Some(io_timeout))
        .and_then(|()| stream.set_write_timeout(Some(io_timeout)))
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to configure machine forwarder timeout {}: {error}",
                config.endpoint_label()
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
                "failed to send machine forwarder {method} request to {}: {error}",
                config.endpoint_label()
            ),
        })?;

    read_machine_forwarder_response(&mut stream, deadline)
}

fn read_machine_forwarder_response(
    stream: &mut MachineForwarderStream,
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

enum MachineForwarderStream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}

impl MachineForwarderStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.set_read_timeout(timeout),
            #[cfg(unix)]
            Self::Unix(stream) => stream.set_read_timeout(timeout),
        }
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.set_write_timeout(timeout),
            #[cfg(unix)]
            Self::Unix(stream) => stream.set_write_timeout(timeout),
        }
    }
}

impl Read for MachineForwarderStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buffer),
            #[cfg(unix)]
            Self::Unix(stream) => stream.read(buffer),
        }
    }
}

impl Write for MachineForwarderStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buffer),
            #[cfg(unix)]
            Self::Unix(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            #[cfg(unix)]
            Self::Unix(stream) => stream.flush(),
        }
    }
}

fn connect_machine_forwarder(
    config: &OciMachinePortForwarderConfig,
    timeout: Duration,
) -> Result<MachineForwarderStream> {
    if let Some(socket_path) = config.unix_socket_path() {
        #[cfg(unix)]
        {
            return connect_unix_with_timeout(socket_path, timeout)
                .map(MachineForwarderStream::Unix)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to connect to machine forwarder services socket {}: {error}",
                        socket_path.display()
                    ),
                });
        }
        #[cfg(not(unix))]
        {
            let _ = timeout;
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine forwarder Unix services socket {} is unavailable on this platform",
                    socket_path.display()
                ),
            });
        }
    }

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
    TcpStream::connect_timeout(&address, timeout)
        .map(MachineForwarderStream::Tcp)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to connect to machine forwarder {}:{}: {error}",
                config.host, config.port
            ),
        })
}

#[cfg(unix)]
fn connect_unix_with_timeout(path: &Path, timeout: Duration) -> std::io::Result<UnixStream> {
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    stream.set_nonblocking(true)?;
    let descriptor_flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFD) };
    if descriptor_flags < 0
        || unsafe {
            libc::fcntl(
                stream.as_raw_fd(),
                libc::F_SETFD,
                descriptor_flags | libc::FD_CLOEXEC,
            )
        } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let path_bytes = path.as_os_str().as_bytes();
    if path_bytes.len() > unix_socket_path_max_bytes() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Unix socket path exceeds the platform limit",
        ));
    }
    let mut address = unsafe { std::mem::zeroed::<libc::sockaddr_un>() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (target, source) in address.sun_path.iter_mut().zip(path_bytes) {
        *target = *source as libc::c_char;
    }
    let address_length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + path_bytes.len() + 1;
    #[cfg(any(
        target_os = "aix",
        target_os = "freebsd",
        target_os = "macos",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    {
        address.sun_len = u8::try_from(address_length).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Unix socket address length exceeds u8",
            )
        })?;
    }
    let connected = unsafe {
        libc::connect(
            stream.as_raw_fd(),
            (&raw const address).cast::<libc::sockaddr>(),
            address_length as libc::socklen_t,
        )
    };
    if connected != 0 {
        let error = std::io::Error::last_os_error();
        if !matches!(
            error.raw_os_error(),
            Some(code) if code == libc::EINPROGRESS
                || code == libc::EAGAIN
                || code == libc::EWOULDBLOCK
        ) {
            return Err(error);
        }
        wait_for_unix_connect(stream.as_raw_fd(), timeout)?;
    }
    stream.set_nonblocking(false)?;
    Ok(stream)
}

#[cfg(unix)]
fn wait_for_unix_connect(fd: std::os::fd::RawFd, timeout: Duration) -> std::io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Unix socket connect timed out",
            ));
        }
        let timeout_ms = remaining
            .as_millis()
            .saturating_add(1)
            .min(i32::MAX as u128) as i32;
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        let result = unsafe { libc::poll(&raw mut poll, 1, timeout_ms) };
        if result == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Unix socket connect timed out",
            ));
        }
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        let mut socket_error = 0;
        let mut socket_error_length = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&raw mut socket_error).cast(),
                &raw mut socket_error_length,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        return if socket_error == 0 {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(socket_error))
        };
    }
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
