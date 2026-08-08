//! Portable compiled network-plan content for one workload generation.

use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::net::IpAddr;
use std::num::NonZeroU16;

use nimbus_core::{TenantId, is_valid_dns_hostname};
use nimbus_network::{
    EndpointProtocol, IngressRouteId, ListenerId, NetworkAttachmentId,
    NetworkCapabilityRequirements, NetworkCapabilitySelection, NetworkCapabilitySelectionEvidence,
    NetworkConditionKind, NetworkPlan, NetworkPlanContentDigest, NetworkPlanId, NetworkProviderId,
    NetworkReadinessRequirement, NetworkReadinessRequirementError, NetworkResourceGeneration,
    NetworkSovereigntyRequirements, NetworkTlsBehavior, PortLeaseId, PublishedEndpointId,
};
use serde::{Deserialize, Serialize};

use crate::{WorkloadActivationIntent, WorkloadPublicationIntent};

/// Portable compiled network-plan content format understood by this crate.
pub const WORKLOAD_NETWORK_PLAN_FORMAT_VERSION: u32 = 2;

/// A rejected portable workload network-plan value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadNetworkPlanError {
    /// A required logical string was empty or contained only whitespace.
    EmptyRequiredField { field: &'static str },
    /// A required logical name was not a concrete whitespace-free value.
    InvalidRequiredField { field: &'static str },
    /// An admitted route host was not a bare DNS name or IP literal.
    InvalidRouteHost { host: String },
    /// A required numeric port was zero.
    ZeroPort { field: &'static str },
    /// An attachment ID did not derive from the retained workload identity.
    AttachmentIdentityMismatch {
        expected: NetworkAttachmentId,
        candidate: NetworkAttachmentId,
    },
    /// A route ID did not derive from the retained workload identity and names.
    RouteIdentityMismatch {
        expected: IngressRouteId,
        candidate: IngressRouteId,
    },
    /// A listener ID did not derive from the retained tenant-qualified identity.
    ListenerIdentityMismatch {
        expected: ListenerId,
        candidate: ListenerId,
    },
    /// A published endpoint ID did not derive from the retained identity and name.
    PublishedEndpointIdentityMismatch {
        expected: PublishedEndpointId,
        candidate: PublishedEndpointId,
    },
    /// A listener carried a lease identity that did not derive from its ID.
    PortLeaseIdentityMismatch {
        expected: PortLeaseId,
        candidate: PortLeaseId,
    },
    /// Two routes used the same service-local logical identity.
    DuplicateRouteName {
        service_name: String,
        route_name: String,
    },
    /// Two routes used the same stable route identity.
    DuplicateRouteId { route_id: IngressRouteId },
    /// Two listeners used the same workload-local logical name.
    DuplicateListenerName { listener_name: String },
    /// Two listeners used the same stable listener identity.
    DuplicateListenerId { listener_id: ListenerId },
    /// Two listeners used the same stable published-endpoint identity.
    DuplicatePublishedEndpointId { endpoint_id: PublishedEndpointId },
    /// Two listeners used the same stable port-lease identity.
    DuplicatePortLeaseId { port_lease_id: PortLeaseId },
    /// A selected provider pair is required to derive resource readiness.
    MissingCapabilitySelectionForResources,
    /// Selected source-report evidence is required for provider-owned resources.
    MissingCapabilitySelectionEvidenceForResources,
    /// A resource-free plan cannot claim that a provider pair was selected.
    UnexpectedCapabilitySelectionForResourceFreePlan,
    /// Selected provider IDs and their source-report evidence disagree.
    CapabilitySelectionEvidenceMismatch,
    /// Listener forwarding does not agree with its guest-port shape.
    ForwardingBehaviorMismatch,
    /// Listener TLS behavior does not agree with its application protocol.
    TlsBehaviorMismatch,
    /// The mechanically derived readiness requirement set was invalid.
    InvalidReadinessRequirements(NetworkReadinessRequirementError),
    /// Serialized content uses an unsupported format version.
    UnsupportedFormatVersion { candidate: u32 },
    /// The plan envelope does not digest the exact retained content.
    ContentDigestMismatch {
        expected: NetworkPlanContentDigest,
        candidate: NetworkPlanContentDigest,
    },
    /// The envelope plan ID did not derive from retained identity.
    PlanIdentityMismatch {
        expected: NetworkPlanId,
        candidate: NetworkPlanId,
    },
    /// The envelope generation did not match retained identity.
    PlanGenerationMismatch {
        expected: NetworkResourceGeneration,
        candidate: NetworkResourceGeneration,
    },
    /// The envelope sovereignty value did not match retained requirements.
    PlanSovereigntyMismatch,
    /// The envelope capability requirements did not match retained requirements.
    PlanCapabilityRequirementsMismatch,
    /// The envelope readiness requirements did not match retained resources.
    PlanReadinessRequirementsMismatch,
}

impl Display for WorkloadNetworkPlanError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRequiredField { field } => {
                write!(
                    formatter,
                    "workload network plan field `{field}` must not be empty"
                )
            }
            Self::InvalidRequiredField { field } => write!(
                formatter,
                "workload network plan field `{field}` must be a concrete whitespace-free value"
            ),
            Self::InvalidRouteHost { host } => write!(
                formatter,
                "workload network plan route host `{host}` must be a bare DNS name or IP literal"
            ),
            Self::ZeroPort { field } => {
                write!(
                    formatter,
                    "workload network plan field `{field}` must not be zero"
                )
            }
            Self::AttachmentIdentityMismatch {
                expected,
                candidate,
            } => write!(
                formatter,
                "workload network attachment ID {candidate} does not match derived ID {expected}"
            ),
            Self::RouteIdentityMismatch {
                expected,
                candidate,
            } => write!(
                formatter,
                "workload network route ID {candidate} does not match derived ID {expected}"
            ),
            Self::ListenerIdentityMismatch {
                expected,
                candidate,
            } => write!(
                formatter,
                "workload network listener ID {candidate} does not match derived ID {expected}"
            ),
            Self::PublishedEndpointIdentityMismatch {
                expected,
                candidate,
            } => write!(
                formatter,
                "workload network endpoint ID {candidate} does not match derived ID {expected}"
            ),
            Self::PortLeaseIdentityMismatch {
                expected,
                candidate,
            } => write!(
                formatter,
                "workload network listener lease ID {candidate} does not match derived ID {expected}"
            ),
            Self::DuplicateRouteName {
                service_name,
                route_name,
            } => write!(
                formatter,
                "workload network plan contains duplicate route `{route_name}` for service `{service_name}`"
            ),
            Self::DuplicateRouteId { route_id } => write!(
                formatter,
                "workload network plan contains duplicate route ID {route_id}"
            ),
            Self::DuplicateListenerName { listener_name } => write!(
                formatter,
                "workload network plan contains duplicate listener `{listener_name}`"
            ),
            Self::DuplicateListenerId { listener_id } => write!(
                formatter,
                "workload network plan contains duplicate listener ID {listener_id}"
            ),
            Self::DuplicatePublishedEndpointId { endpoint_id } => write!(
                formatter,
                "workload network plan contains duplicate published endpoint ID {endpoint_id}"
            ),
            Self::DuplicatePortLeaseId { port_lease_id } => write!(
                formatter,
                "workload network plan contains duplicate port lease ID {port_lease_id}"
            ),
            Self::MissingCapabilitySelectionForResources => formatter
                .write_str("workload network resources require an exact capability selection"),
            Self::MissingCapabilitySelectionEvidenceForResources => formatter.write_str(
                "workload network resources require exact capability selection evidence",
            ),
            Self::UnexpectedCapabilitySelectionForResourceFreePlan => formatter.write_str(
                "a resource-free workload network plan cannot select provider capabilities",
            ),
            Self::CapabilitySelectionEvidenceMismatch => formatter.write_str(
                "workload network capability selection does not match its source evidence",
            ),
            Self::ForwardingBehaviorMismatch => {
                formatter.write_str("listener forwarding behavior must match guest port shape")
            }
            Self::TlsBehaviorMismatch => {
                formatter.write_str("listener TLS behavior must match its application protocol")
            }
            Self::InvalidReadinessRequirements(error) => {
                write!(formatter, "workload network readiness is invalid: {error}")
            }
            Self::UnsupportedFormatVersion { candidate } => write!(
                formatter,
                "workload network plan format version {candidate} is unsupported; expected {WORKLOAD_NETWORK_PLAN_FORMAT_VERSION}"
            ),
            Self::ContentDigestMismatch {
                expected,
                candidate,
            } => write!(
                formatter,
                "workload network plan content digest {candidate} does not match envelope digest {expected}"
            ),
            Self::PlanIdentityMismatch {
                expected,
                candidate,
            } => write!(
                formatter,
                "workload network plan ID {candidate} does not match derived ID {expected}"
            ),
            Self::PlanGenerationMismatch {
                expected,
                candidate,
            } => write!(
                formatter,
                "workload network plan generation {} does not match retained generation {}",
                candidate.as_u64(),
                expected.as_u64()
            ),
            Self::PlanSovereigntyMismatch => formatter.write_str(
                "workload network plan sovereignty does not match retained requirements",
            ),
            Self::PlanCapabilityRequirementsMismatch => formatter
                .write_str("workload network plan capabilities do not match retained requirements"),
            Self::PlanReadinessRequirementsMismatch => formatter
                .write_str("workload network plan readiness does not match retained resources"),
        }
    }
}

impl StdError for WorkloadNetworkPlanError {}

fn validate_required(value: &str, field: &'static str) -> Result<(), WorkloadNetworkPlanError> {
    if value.trim().is_empty() {
        return Err(WorkloadNetworkPlanError::EmptyRequiredField { field });
    }
    if value == "*" || value != value.trim() || value.contains(char::is_whitespace) {
        return Err(WorkloadNetworkPlanError::InvalidRequiredField { field });
    }
    Ok(())
}

fn validate_route_host(host: &str) -> Result<(), WorkloadNetworkPlanError> {
    validate_required(host, "route.host")?;
    let invalid_delimiter = host.contains("://")
        || host.contains('/')
        || host.contains('\\')
        || host.contains('@')
        || host.starts_with('[')
        || host.ends_with(']');
    if invalid_delimiter {
        return Err(WorkloadNetworkPlanError::InvalidRouteHost {
            host: host.to_owned(),
        });
    }
    if let Ok(address) = host.parse::<IpAddr>() {
        if !address.is_unspecified() {
            return Ok(());
        }
    } else if !host.contains(':') && is_valid_dns_hostname(host) {
        return Ok(());
    }
    Err(WorkloadNetworkPlanError::InvalidRouteHost {
        host: host.to_owned(),
    })
}

fn validate_optional_port(
    port: Option<u16>,
    field: &'static str,
) -> Result<(), WorkloadNetworkPlanError> {
    if port == Some(0) {
        Err(WorkloadNetworkPlanError::ZeroPort { field })
    } else {
        Ok(())
    }
}

/// Stable tenant-qualified identity retained by one workload network plan.
///
/// The admitted workload incarnation key is control-plane identity, not an IP
/// address, numeric port, provider handle, or allocation epoch. Every resource
/// identity in portable content is rederived from this value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadNetworkPlanIdentity {
    tenant_id: TenantId,
    workload_incarnation_key: String,
    generation: NetworkResourceGeneration,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadNetworkPlanIdentityWire {
    tenant_id: TenantId,
    workload_incarnation_key: String,
    generation: NetworkResourceGeneration,
}

impl WorkloadNetworkPlanIdentity {
    /// Construct one retained workload incarnation identity.
    pub fn new(
        tenant_id: TenantId,
        workload_incarnation_key: impl Into<String>,
        generation: NetworkResourceGeneration,
    ) -> Result<Self, WorkloadNetworkPlanError> {
        let workload_incarnation_key = workload_incarnation_key.into();
        validate_required(
            &workload_incarnation_key,
            "identity.workload_incarnation_key",
        )?;
        Ok(Self {
            tenant_id,
            workload_incarnation_key,
            generation,
        })
    }

    /// Tenant that owns every resource in the plan.
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Stable replacement-fenced workload incarnation key.
    pub fn workload_incarnation_key(&self) -> &str {
        &self.workload_incarnation_key
    }

    /// Desired generation retained independently of the envelope.
    pub const fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }

    /// Stable identity of the complete desired network plan.
    pub fn plan_id(&self) -> NetworkPlanId {
        NetworkPlanId::for_tenant_workload_plan(&self.tenant_id, &self.workload_incarnation_key)
    }

    fn resource_incarnation_key(&self) -> String {
        format!(
            "nimbus.network.tenant-workload-incarnation.v1:{}:{}:{}:{}",
            self.tenant_id.as_str().len(),
            self.tenant_id.as_str(),
            self.workload_incarnation_key.len(),
            self.workload_incarnation_key,
        )
    }

    fn attachment_id(&self, name: &str) -> NetworkAttachmentId {
        NetworkAttachmentId::for_workload_attachment(&self.resource_incarnation_key(), name)
    }

    fn route_id(&self, service_name: &str, route_name: &str) -> IngressRouteId {
        IngressRouteId::for_workload_route(
            &self.resource_incarnation_key(),
            service_name,
            route_name,
        )
    }

    fn listener_id(&self, name: &str) -> ListenerId {
        ListenerId::for_tenant_workload_listener(
            &self.tenant_id,
            &self.workload_incarnation_key,
            name,
        )
    }

    fn endpoint_id(&self, name: &str) -> PublishedEndpointId {
        PublishedEndpointId::for_workload_endpoint(&self.resource_incarnation_key(), name)
    }
}

impl<'de> Deserialize<'de> for WorkloadNetworkPlanIdentity {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let wire = WorkloadNetworkPlanIdentityWire::deserialize(deserializer)?;
        Self::new(
            wire.tenant_id,
            wire.workload_incarnation_key,
            wire.generation,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// One named workload-to-network attachment in compiled desired content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadNetworkAttachmentBlueprint {
    attachment_id: NetworkAttachmentId,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadNetworkAttachmentBlueprintWire {
    attachment_id: NetworkAttachmentId,
    name: String,
}

impl WorkloadNetworkAttachmentBlueprint {
    /// Derive one validated attachment blueprint from retained identity.
    pub fn new(
        identity: &WorkloadNetworkPlanIdentity,
        name: impl Into<String>,
    ) -> Result<Self, WorkloadNetworkPlanError> {
        let name = name.into();
        validate_required(&name, "attachment.name")?;
        Ok(Self {
            attachment_id: identity.attachment_id(&name),
            name,
        })
    }

    fn from_wire(
        attachment_id: NetworkAttachmentId,
        name: String,
    ) -> Result<Self, WorkloadNetworkPlanError> {
        validate_required(&name, "attachment.name")?;
        Ok(Self {
            attachment_id,
            name,
        })
    }

    /// Stable attachment identity.
    pub fn attachment_id(&self) -> &NetworkAttachmentId {
        &self.attachment_id
    }

    /// Workload-local logical attachment name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl WorkloadNetworkAttachmentBlueprintWire {
    fn into_blueprint(
        self,
    ) -> Result<WorkloadNetworkAttachmentBlueprint, WorkloadNetworkPlanError> {
        WorkloadNetworkAttachmentBlueprint::from_wire(self.attachment_id, self.name)
    }
}

/// One admitted service route in compiled desired content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadNetworkRouteBlueprint {
    route_id: IngressRouteId,
    service_name: String,
    route_name: String,
    protocol: EndpointProtocol,
    host: String,
    host_port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    guest_port: Option<u16>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadNetworkRouteBlueprintWire {
    route_id: IngressRouteId,
    service_name: String,
    route_name: String,
    protocol: EndpointProtocol,
    host: String,
    host_port: u16,
    #[serde(default)]
    guest_port: Option<u16>,
}

impl WorkloadNetworkRouteBlueprint {
    /// Derive one validated admitted route blueprint from retained identity.
    pub fn new(
        identity: &WorkloadNetworkPlanIdentity,
        service_name: impl Into<String>,
        route_name: impl Into<String>,
        protocol: EndpointProtocol,
        host: impl Into<String>,
        host_port: u16,
        guest_port: Option<u16>,
    ) -> Result<Self, WorkloadNetworkPlanError> {
        let service_name = service_name.into();
        let route_name = route_name.into();
        let host = host.into();
        validate_required(&service_name, "route.service_name")?;
        validate_required(&route_name, "route.route_name")?;
        validate_route_host(&host)?;
        if host_port == 0 {
            return Err(WorkloadNetworkPlanError::ZeroPort {
                field: "route.host_port",
            });
        }
        validate_optional_port(guest_port, "route.guest_port")?;
        let route_id = identity.route_id(&service_name, &route_name);
        Ok(Self {
            route_id,
            service_name,
            route_name,
            protocol,
            host,
            host_port,
            guest_port,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn from_wire(
        route_id: IngressRouteId,
        service_name: String,
        route_name: String,
        protocol: EndpointProtocol,
        host: String,
        host_port: u16,
        guest_port: Option<u16>,
    ) -> Result<Self, WorkloadNetworkPlanError> {
        validate_required(&service_name, "route.service_name")?;
        validate_required(&route_name, "route.route_name")?;
        validate_route_host(&host)?;
        if host_port == 0 {
            return Err(WorkloadNetworkPlanError::ZeroPort {
                field: "route.host_port",
            });
        }
        validate_optional_port(guest_port, "route.guest_port")?;
        Ok(Self {
            route_id,
            service_name,
            route_name,
            protocol,
            host,
            host_port,
            guest_port,
        })
    }

    /// Stable route identity.
    pub fn route_id(&self) -> &IngressRouteId {
        &self.route_id
    }

    /// Services-owned logical service name.
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// Service-local logical route name.
    pub fn route_name(&self) -> &str {
        &self.route_name
    }

    /// Admitted application protocol.
    pub const fn protocol(&self) -> EndpointProtocol {
        self.protocol
    }

    /// Admitted route host correlation value.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Admitted route host port.
    pub const fn host_port(&self) -> u16 {
        self.host_port
    }

    /// Optional guest-side port correlation value.
    pub const fn guest_port(&self) -> Option<u16> {
        self.guest_port
    }
}

impl WorkloadNetworkRouteBlueprintWire {
    fn into_blueprint(self) -> Result<WorkloadNetworkRouteBlueprint, WorkloadNetworkPlanError> {
        WorkloadNetworkRouteBlueprint::from_wire(
            self.route_id,
            self.service_name,
            self.route_name,
            self.protocol,
            self.host,
            self.host_port,
            self.guest_port,
        )
    }
}

/// Numeric port-selection intent retained before lease reservation.
///
/// A range request belongs to allocation policy and is not admitted into this
/// exact workload payload. A reservation authority assigns every lease epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkloadNetworkPortRequestMode {
    /// Reserve this exact non-zero host port.
    Exact { port: NonZeroU16 },
    /// Let the effect owner request port zero and report the selected port.
    ProviderAssigned,
}

/// Exact forwarding intent for one workload listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadNetworkForwardingBehavior {
    /// The admitted endpoint needs no forwarding hop.
    None,
    /// The host-side listener forwards to the retained guest port.
    PortForwarded,
}

/// Explicit forwarding and TLS semantics bound to one named listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadNetworkEndpointSemantics {
    forwarding: WorkloadNetworkForwardingBehavior,
    tls: NetworkTlsBehavior,
}

impl WorkloadNetworkEndpointSemantics {
    /// Construct exact endpoint semantics.
    pub const fn new(
        forwarding: WorkloadNetworkForwardingBehavior,
        tls: NetworkTlsBehavior,
    ) -> Self {
        Self { forwarding, tls }
    }

    /// Admitted forwarding behavior.
    pub const fn forwarding(self) -> WorkloadNetworkForwardingBehavior {
        self.forwarding
    }

    /// Admitted TLS handling behavior.
    pub const fn tls(self) -> NetworkTlsBehavior {
        self.tls
    }
}

impl WorkloadNetworkPortRequestMode {
    /// Construct exact non-zero port intent.
    pub const fn exact(port: NonZeroU16) -> Self {
        Self::Exact { port }
    }

    /// Requested exact port, or `None` for provider assignment.
    pub const fn exact_port(self) -> Option<NonZeroU16> {
        match self {
            Self::Exact { port } => Some(port),
            Self::ProviderAssigned => None,
        }
    }
}

/// One named listener and its pre-reservation publication intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadNetworkListenerBlueprint {
    listener_id: ListenerId,
    endpoint_id: PublishedEndpointId,
    port_lease_id: PortLeaseId,
    name: String,
    protocol: EndpointProtocol,
    desired_host_address: IpAddr,
    port_request: WorkloadNetworkPortRequestMode,
    endpoint_semantics: WorkloadNetworkEndpointSemantics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    guest_port: Option<u16>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadNetworkListenerBlueprintWire {
    listener_id: ListenerId,
    endpoint_id: PublishedEndpointId,
    port_lease_id: PortLeaseId,
    name: String,
    protocol: EndpointProtocol,
    desired_host_address: IpAddr,
    port_request: WorkloadNetworkPortRequestMode,
    endpoint_semantics: WorkloadNetworkEndpointSemantics,
    #[serde(default)]
    guest_port: Option<u16>,
}

impl WorkloadNetworkListenerBlueprint {
    /// Derive one validated listener blueprint from retained identity.
    pub fn new(
        identity: &WorkloadNetworkPlanIdentity,
        name: impl Into<String>,
        protocol: EndpointProtocol,
        desired_host_address: IpAddr,
        port_request: WorkloadNetworkPortRequestMode,
        endpoint_semantics: WorkloadNetworkEndpointSemantics,
        guest_port: Option<u16>,
    ) -> Result<Self, WorkloadNetworkPlanError> {
        let name = name.into();
        validate_required(&name, "listener.name")?;
        validate_optional_port(guest_port, "listener.guest_port")?;
        validate_endpoint_semantics(protocol, endpoint_semantics, guest_port)?;
        let listener_id = identity.listener_id(&name);
        let endpoint_id = identity.endpoint_id(&name);
        let port_lease_id = PortLeaseId::for_listener(&listener_id);
        Ok(Self {
            listener_id,
            endpoint_id,
            port_lease_id,
            name,
            protocol,
            desired_host_address,
            port_request,
            endpoint_semantics,
            guest_port,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn from_wire(
        listener_id: ListenerId,
        endpoint_id: PublishedEndpointId,
        port_lease_id: PortLeaseId,
        name: String,
        protocol: EndpointProtocol,
        desired_host_address: IpAddr,
        port_request: WorkloadNetworkPortRequestMode,
        endpoint_semantics: WorkloadNetworkEndpointSemantics,
        guest_port: Option<u16>,
    ) -> Result<Self, WorkloadNetworkPlanError> {
        validate_required(&name, "listener.name")?;
        validate_optional_port(guest_port, "listener.guest_port")?;
        validate_endpoint_semantics(protocol, endpoint_semantics, guest_port)?;
        let expected_lease_id = PortLeaseId::for_listener(&listener_id);
        if port_lease_id != expected_lease_id {
            return Err(WorkloadNetworkPlanError::PortLeaseIdentityMismatch {
                expected: expected_lease_id,
                candidate: port_lease_id,
            });
        }
        Ok(Self {
            listener_id,
            endpoint_id,
            port_lease_id,
            name,
            protocol,
            desired_host_address,
            port_request,
            endpoint_semantics,
            guest_port,
        })
    }

    /// Stable address-independent listener identity.
    pub fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }

    /// Stable address-independent published-endpoint identity.
    pub fn endpoint_id(&self) -> &PublishedEndpointId {
        &self.endpoint_id
    }

    /// Stable address-independent lease identity.
    pub fn port_lease_id(&self) -> &PortLeaseId {
        &self.port_lease_id
    }

    /// Workload-local logical listener name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Admitted application protocol.
    pub const fn protocol(&self) -> EndpointProtocol {
        self.protocol
    }

    /// Desired publication address, which is never resource identity.
    pub const fn desired_host_address(&self) -> IpAddr {
        self.desired_host_address
    }

    /// Exact or provider-assigned pre-reservation port intent.
    pub const fn port_request(&self) -> WorkloadNetworkPortRequestMode {
        self.port_request
    }

    /// Exact forwarding and TLS semantics for this listener.
    pub const fn endpoint_semantics(&self) -> WorkloadNetworkEndpointSemantics {
        self.endpoint_semantics
    }

    /// Optional guest-side port correlation value.
    pub const fn guest_port(&self) -> Option<u16> {
        self.guest_port
    }
}

impl WorkloadNetworkListenerBlueprintWire {
    fn into_blueprint(self) -> Result<WorkloadNetworkListenerBlueprint, WorkloadNetworkPlanError> {
        WorkloadNetworkListenerBlueprint::from_wire(
            self.listener_id,
            self.endpoint_id,
            self.port_lease_id,
            self.name,
            self.protocol,
            self.desired_host_address,
            self.port_request,
            self.endpoint_semantics,
            self.guest_port,
        )
    }
}

fn validate_endpoint_semantics(
    protocol: EndpointProtocol,
    endpoint_semantics: WorkloadNetworkEndpointSemantics,
    guest_port: Option<u16>,
) -> Result<(), WorkloadNetworkPlanError> {
    let forwarding_matches = matches!(
        (endpoint_semantics.forwarding(), guest_port),
        (WorkloadNetworkForwardingBehavior::None, None)
            | (WorkloadNetworkForwardingBehavior::PortForwarded, Some(_))
    );
    if !forwarding_matches {
        return Err(WorkloadNetworkPlanError::ForwardingBehaviorMismatch);
    }
    let tls_matches = matches!(
        (protocol, endpoint_semantics.tls()),
        (
            EndpointProtocol::Tcp | EndpointProtocol::Http,
            NetworkTlsBehavior::Disabled
        ) | (
            EndpointProtocol::Https,
            NetworkTlsBehavior::Passthrough | NetworkTlsBehavior::TerminateAtIngress
        )
    );
    if !tls_matches {
        return Err(WorkloadNetworkPlanError::TlsBehaviorMismatch);
    }
    Ok(())
}

/// One non-published listener whose readiness gates workload activation.
///
/// Compute owns the decision to include this dependency. This portable value
/// only retains the tenant-qualified listener identity and exact evidence
/// provider needed to reconstruct the plan envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadNetworkDependencyListenerBlueprint {
    listener_id: ListenerId,
    name: String,
    provider_id: NetworkProviderId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadNetworkDependencyListenerBlueprintWire {
    listener_id: ListenerId,
    name: String,
    provider_id: NetworkProviderId,
}

impl WorkloadNetworkDependencyListenerBlueprint {
    /// Derive one exact readiness dependency from retained identity.
    pub fn new(
        identity: &WorkloadNetworkPlanIdentity,
        name: impl Into<String>,
        provider_id: NetworkProviderId,
    ) -> Result<Self, WorkloadNetworkPlanError> {
        let name = name.into();
        validate_required(&name, "dependency_listener.name")?;
        Ok(Self {
            listener_id: identity.listener_id(&name),
            name,
            provider_id,
        })
    }

    fn from_wire(
        listener_id: ListenerId,
        name: String,
        provider_id: NetworkProviderId,
    ) -> Result<Self, WorkloadNetworkPlanError> {
        validate_required(&name, "dependency_listener.name")?;
        Ok(Self {
            listener_id,
            name,
            provider_id,
        })
    }

    /// Stable listener dependency identity.
    pub fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }

    /// Workload-local dependency name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Exact provider registration expected to report readiness.
    pub fn provider_id(&self) -> &NetworkProviderId {
        &self.provider_id
    }
}

impl WorkloadNetworkDependencyListenerBlueprintWire {
    fn into_blueprint(
        self,
    ) -> Result<WorkloadNetworkDependencyListenerBlueprint, WorkloadNetworkPlanError> {
        WorkloadNetworkDependencyListenerBlueprint::from_wire(
            self.listener_id,
            self.name,
            self.provider_id,
        )
    }
}

/// Exact portable network resources retained for one workload generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadNetworkPlanContent {
    format_version: u32,
    identity: WorkloadNetworkPlanIdentity,
    capability_requirements: NetworkCapabilityRequirements,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    capability_selection: Option<NetworkCapabilitySelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    capability_selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attachment: Option<WorkloadNetworkAttachmentBlueprint>,
    routes: Vec<WorkloadNetworkRouteBlueprint>,
    listeners: Vec<WorkloadNetworkListenerBlueprint>,
    dependency_listeners: Vec<WorkloadNetworkDependencyListenerBlueprint>,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadNetworkPlanContentWire {
    format_version: u32,
    identity: WorkloadNetworkPlanIdentity,
    capability_requirements: NetworkCapabilityRequirements,
    #[serde(default)]
    capability_selection: Option<NetworkCapabilitySelection>,
    #[serde(default)]
    capability_selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
    #[serde(default)]
    attachment: Option<WorkloadNetworkAttachmentBlueprintWire>,
    routes: Vec<WorkloadNetworkRouteBlueprintWire>,
    listeners: Vec<WorkloadNetworkListenerBlueprintWire>,
    dependency_listeners: Vec<WorkloadNetworkDependencyListenerBlueprintWire>,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
}

impl WorkloadNetworkPlanContent {
    /// Construct canonical, duplicate-free portable content.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        identity: WorkloadNetworkPlanIdentity,
        capability_requirements: NetworkCapabilityRequirements,
        capability_selection: Option<NetworkCapabilitySelection>,
        capability_selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
        attachment: Option<WorkloadNetworkAttachmentBlueprint>,
        routes: impl IntoIterator<Item = WorkloadNetworkRouteBlueprint>,
        listeners: impl IntoIterator<Item = WorkloadNetworkListenerBlueprint>,
        dependency_listeners: impl IntoIterator<Item = WorkloadNetworkDependencyListenerBlueprint>,
        activation: WorkloadActivationIntent,
        publication: WorkloadPublicationIntent,
    ) -> Result<Self, WorkloadNetworkPlanError> {
        let mut routes: Vec<_> = routes.into_iter().collect();
        validate_routes(&identity, &routes)?;
        routes.sort_by(|first, second| {
            (
                first.service_name.as_str(),
                first.route_name.as_str(),
                first.route_id.as_str(),
            )
                .cmp(&(
                    second.service_name.as_str(),
                    second.route_name.as_str(),
                    second.route_id.as_str(),
                ))
        });

        let mut listeners: Vec<_> = listeners.into_iter().collect();
        validate_listeners(&identity, &listeners)?;
        listeners.sort_by(|first, second| {
            (first.name.as_str(), first.listener_id.as_str())
                .cmp(&(second.name.as_str(), second.listener_id.as_str()))
        });

        let mut dependency_listeners: Vec<_> = dependency_listeners.into_iter().collect();
        validate_dependency_listeners(&identity, &listeners, &dependency_listeners)?;
        dependency_listeners.sort_by(|first, second| {
            (first.name.as_str(), first.listener_id.as_str())
                .cmp(&(second.name.as_str(), second.listener_id.as_str()))
        });

        if let Some(attachment) = attachment.as_ref() {
            let expected = identity.attachment_id(attachment.name());
            if attachment.attachment_id != expected {
                return Err(WorkloadNetworkPlanError::AttachmentIdentityMismatch {
                    expected,
                    candidate: attachment.attachment_id.clone(),
                });
            }
        }
        let has_selected_resources =
            attachment.is_some() || !routes.is_empty() || !listeners.is_empty();
        match (
            has_selected_resources,
            capability_selection.as_ref(),
            capability_selection_evidence.as_ref(),
        ) {
            (true, None, _) => {
                return Err(WorkloadNetworkPlanError::MissingCapabilitySelectionForResources);
            }
            (true, Some(_), None) => {
                return Err(
                    WorkloadNetworkPlanError::MissingCapabilitySelectionEvidenceForResources,
                );
            }
            (true, Some(selection), Some(evidence)) if evidence.selection() != selection => {
                return Err(WorkloadNetworkPlanError::CapabilitySelectionEvidenceMismatch);
            }
            (false, None, None) => {}
            (false, _, _) => {
                return Err(
                    WorkloadNetworkPlanError::UnexpectedCapabilitySelectionForResourceFreePlan,
                );
            }
            (true, Some(_), Some(_)) => {}
        }

        Ok(Self {
            format_version: WORKLOAD_NETWORK_PLAN_FORMAT_VERSION,
            identity,
            capability_requirements,
            capability_selection,
            capability_selection_evidence,
            attachment,
            routes,
            listeners,
            dependency_listeners,
            activation,
            publication,
        })
    }

    /// Portable content format version.
    pub const fn format_version(&self) -> u32 {
        self.format_version
    }

    /// Tenant-qualified workload identity and desired generation.
    pub fn identity(&self) -> &WorkloadNetworkPlanIdentity {
        &self.identity
    }

    /// Exact complete capability requirements retained by the payload.
    pub fn capability_requirements(&self) -> &NetworkCapabilityRequirements {
        &self.capability_requirements
    }

    /// Exact provider registrations selected by the upper composition owner.
    pub fn capability_selection(&self) -> Option<&NetworkCapabilitySelection> {
        self.capability_selection.as_ref()
    }

    /// Authenticated source reports behind the exact selected providers.
    pub fn capability_selection_evidence(&self) -> Option<&NetworkCapabilitySelectionEvidence> {
        self.capability_selection_evidence.as_ref()
    }

    /// Explicit admitted sovereignty constraints.
    pub fn sovereignty_requirements(&self) -> &NetworkSovereigntyRequirements {
        self.capability_requirements.sovereignty()
    }

    /// Optional workload attachment.
    pub fn attachment(&self) -> Option<&WorkloadNetworkAttachmentBlueprint> {
        self.attachment.as_ref()
    }

    /// Canonically ordered admitted service routes.
    pub fn routes(&self) -> &[WorkloadNetworkRouteBlueprint] {
        &self.routes
    }

    /// Canonically ordered listener blueprints.
    pub fn listeners(&self) -> &[WorkloadNetworkListenerBlueprint] {
        &self.listeners
    }

    /// Canonically ordered non-published listener readiness dependencies.
    pub fn dependency_listeners(&self) -> &[WorkloadNetworkDependencyListenerBlueprint] {
        &self.dependency_listeners
    }

    /// Workload activation intent bound into the plan digest.
    pub const fn activation(&self) -> WorkloadActivationIntent {
        self.activation
    }

    /// Workload publication intent bound into the plan digest.
    pub const fn publication(&self) -> WorkloadPublicationIntent {
        self.publication
    }

    /// Serialize the exact retained content into its canonical digest input.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self)
            .expect("validated portable workload network-plan content always serializes")
    }

    fn readiness_requirements(
        &self,
    ) -> Result<Vec<NetworkReadinessRequirement>, WorkloadNetworkPlanError> {
        let mut readiness = Vec::with_capacity(
            usize::from(self.attachment.is_some())
                + self.listeners.len()
                + self.dependency_listeners.len(),
        );
        if self.attachment.is_some() || !self.listeners.is_empty() {
            let selection = self
                .capability_selection
                .as_ref()
                .ok_or(WorkloadNetworkPlanError::MissingCapabilitySelectionForResources)?;
            if let Some(attachment) = self.attachment.as_ref() {
                readiness.push(NetworkReadinessRequirement::new(
                    attachment.attachment_id.clone().into(),
                    selection.attachment_provider_id().clone(),
                    NetworkConditionKind::Ready,
                ));
            }
            readiness.extend(self.listeners.iter().map(|listener| {
                NetworkReadinessRequirement::new(
                    listener.listener_id.clone().into(),
                    selection.ingress_provider_id().clone(),
                    NetworkConditionKind::Ready,
                )
            }));
        }
        readiness.extend(self.dependency_listeners.iter().map(|dependency| {
            NetworkReadinessRequirement::new(
                dependency.listener_id.clone().into(),
                dependency.provider_id.clone(),
                NetworkConditionKind::Ready,
            )
        }));
        readiness.sort();
        if let Some(requirement) = readiness
            .windows(2)
            .find(|pair| pair[0] == pair[1])
            .map(|pair| pair[0].clone())
        {
            return Err(WorkloadNetworkPlanError::InvalidReadinessRequirements(
                NetworkReadinessRequirementError::Duplicate { requirement },
            ));
        }
        Ok(readiness)
    }
}

impl<'de> Deserialize<'de> for WorkloadNetworkPlanContent {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let wire = WorkloadNetworkPlanContentWire::deserialize(deserializer)?;
        if wire.format_version != WORKLOAD_NETWORK_PLAN_FORMAT_VERSION {
            return Err(serde::de::Error::custom(
                WorkloadNetworkPlanError::UnsupportedFormatVersion {
                    candidate: wire.format_version,
                },
            ));
        }
        let attachment = wire
            .attachment
            .map(WorkloadNetworkAttachmentBlueprintWire::into_blueprint)
            .transpose()
            .map_err(serde::de::Error::custom)?;
        let routes = wire
            .routes
            .into_iter()
            .map(WorkloadNetworkRouteBlueprintWire::into_blueprint)
            .collect::<Result<Vec<_>, _>>()
            .map_err(serde::de::Error::custom)?;
        let listeners = wire
            .listeners
            .into_iter()
            .map(WorkloadNetworkListenerBlueprintWire::into_blueprint)
            .collect::<Result<Vec<_>, _>>()
            .map_err(serde::de::Error::custom)?;
        let dependency_listeners = wire
            .dependency_listeners
            .into_iter()
            .map(WorkloadNetworkDependencyListenerBlueprintWire::into_blueprint)
            .collect::<Result<Vec<_>, _>>()
            .map_err(serde::de::Error::custom)?;
        Self::new(
            wire.identity,
            wire.capability_requirements,
            wire.capability_selection,
            wire.capability_selection_evidence,
            attachment,
            routes,
            listeners,
            dependency_listeners,
            wire.activation,
            wire.publication,
        )
        .map_err(serde::de::Error::custom)
    }
}

fn validate_routes(
    identity: &WorkloadNetworkPlanIdentity,
    routes: &[WorkloadNetworkRouteBlueprint],
) -> Result<(), WorkloadNetworkPlanError> {
    let mut logical_ids = BTreeSet::new();
    let mut stable_ids = BTreeSet::new();
    for route in routes {
        let logical_id = (route.service_name.clone(), route.route_name.clone());
        if !logical_ids.insert(logical_id.clone()) {
            return Err(WorkloadNetworkPlanError::DuplicateRouteName {
                service_name: logical_id.0,
                route_name: logical_id.1,
            });
        }
        if !stable_ids.insert(route.route_id.clone()) {
            return Err(WorkloadNetworkPlanError::DuplicateRouteId {
                route_id: route.route_id.clone(),
            });
        }
        let expected = identity.route_id(&route.service_name, &route.route_name);
        if route.route_id != expected {
            return Err(WorkloadNetworkPlanError::RouteIdentityMismatch {
                expected,
                candidate: route.route_id.clone(),
            });
        }
    }
    Ok(())
}

fn validate_listeners(
    identity: &WorkloadNetworkPlanIdentity,
    listeners: &[WorkloadNetworkListenerBlueprint],
) -> Result<(), WorkloadNetworkPlanError> {
    let mut logical_names = BTreeSet::new();
    let mut listener_ids = BTreeSet::new();
    let mut endpoint_ids = BTreeSet::new();
    let mut port_lease_ids = BTreeSet::new();
    for listener in listeners {
        if !logical_names.insert(listener.name.clone()) {
            return Err(WorkloadNetworkPlanError::DuplicateListenerName {
                listener_name: listener.name.clone(),
            });
        }
        if !listener_ids.insert(listener.listener_id.clone()) {
            return Err(WorkloadNetworkPlanError::DuplicateListenerId {
                listener_id: listener.listener_id.clone(),
            });
        }
        if !endpoint_ids.insert(listener.endpoint_id.clone()) {
            return Err(WorkloadNetworkPlanError::DuplicatePublishedEndpointId {
                endpoint_id: listener.endpoint_id.clone(),
            });
        }
        if !port_lease_ids.insert(listener.port_lease_id.clone()) {
            return Err(WorkloadNetworkPlanError::DuplicatePortLeaseId {
                port_lease_id: listener.port_lease_id.clone(),
            });
        }
        let expected_listener_id = identity.listener_id(&listener.name);
        if listener.listener_id != expected_listener_id {
            return Err(WorkloadNetworkPlanError::ListenerIdentityMismatch {
                expected: expected_listener_id,
                candidate: listener.listener_id.clone(),
            });
        }
        let expected_endpoint_id = identity.endpoint_id(&listener.name);
        if listener.endpoint_id != expected_endpoint_id {
            return Err(
                WorkloadNetworkPlanError::PublishedEndpointIdentityMismatch {
                    expected: expected_endpoint_id,
                    candidate: listener.endpoint_id.clone(),
                },
            );
        }
        let expected_lease_id = PortLeaseId::for_listener(&listener.listener_id);
        if listener.port_lease_id != expected_lease_id {
            return Err(WorkloadNetworkPlanError::PortLeaseIdentityMismatch {
                expected: expected_lease_id,
                candidate: listener.port_lease_id.clone(),
            });
        }
    }
    Ok(())
}

fn validate_dependency_listeners(
    identity: &WorkloadNetworkPlanIdentity,
    listeners: &[WorkloadNetworkListenerBlueprint],
    dependencies: &[WorkloadNetworkDependencyListenerBlueprint],
) -> Result<(), WorkloadNetworkPlanError> {
    let mut names = listeners
        .iter()
        .map(|listener| listener.name.clone())
        .collect::<BTreeSet<_>>();
    let mut ids = listeners
        .iter()
        .map(|listener| listener.listener_id.clone())
        .collect::<BTreeSet<_>>();
    for dependency in dependencies {
        let expected = identity.listener_id(&dependency.name);
        if dependency.listener_id != expected {
            return Err(WorkloadNetworkPlanError::ListenerIdentityMismatch {
                expected,
                candidate: dependency.listener_id.clone(),
            });
        }
        if !names.insert(dependency.name.clone()) {
            return Err(WorkloadNetworkPlanError::DuplicateListenerName {
                listener_name: dependency.name.clone(),
            });
        }
        if !ids.insert(dependency.listener_id.clone()) {
            return Err(WorkloadNetworkPlanError::DuplicateListenerId {
                listener_id: dependency.listener_id.clone(),
            });
        }
    }
    Ok(())
}

/// A desired [`NetworkPlan`] authenticated against its retained portable content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledWorkloadNetworkPlan {
    plan: NetworkPlan,
    content: WorkloadNetworkPlanContent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CompiledWorkloadNetworkPlanWire {
    plan: NetworkPlan,
    content: WorkloadNetworkPlanContent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SagaNetworkPlanWire {
    plan_id: NetworkPlanId,
    #[serde(deserialize_with = "deserialize_saga_network_generation")]
    generation: NetworkResourceGeneration,
    content_digest: NetworkPlanContentDigest,
    requirements: NetworkCapabilityRequirements,
    readiness_requirements: Vec<NetworkReadinessRequirement>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SagaWorkloadNetworkPlanIdentityWire {
    tenant_id: TenantId,
    workload_incarnation_key: String,
    #[serde(deserialize_with = "deserialize_saga_network_generation")]
    generation: NetworkResourceGeneration,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SagaWorkloadNetworkPlanContentWire {
    format_version: u32,
    identity: SagaWorkloadNetworkPlanIdentityWire,
    capability_requirements: NetworkCapabilityRequirements,
    #[serde(default)]
    capability_selection: Option<NetworkCapabilitySelection>,
    #[serde(default)]
    capability_selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
    #[serde(default)]
    attachment: Option<WorkloadNetworkAttachmentBlueprintWire>,
    routes: Vec<WorkloadNetworkRouteBlueprintWire>,
    listeners: Vec<WorkloadNetworkListenerBlueprintWire>,
    dependency_listeners: Vec<WorkloadNetworkDependencyListenerBlueprintWire>,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SagaCompiledWorkloadNetworkPlanWire {
    plan: SagaNetworkPlanWire,
    content: SagaWorkloadNetworkPlanContentWire,
}

fn deserialize_saga_network_generation<'de, Deserializer>(
    deserializer: Deserializer,
) -> Result<NetworkResourceGeneration, Deserializer::Error>
where
    Deserializer: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || value.bytes().any(|byte| !byte.is_ascii_digit())
    {
        return Err(serde::de::Error::custom(
            "network generation must be canonical unsigned decimal text",
        ));
    }
    value
        .parse()
        .map(NetworkResourceGeneration::new)
        .map_err(|_| {
            serde::de::Error::custom("network generation must be canonical unsigned decimal text")
        })
}

pub(crate) fn deserialize_saga_compiled_network_plan<'de, Deserializer>(
    deserializer: Deserializer,
) -> Result<CompiledWorkloadNetworkPlan, Deserializer::Error>
where
    Deserializer: serde::Deserializer<'de>,
{
    let wire = SagaCompiledWorkloadNetworkPlanWire::deserialize(deserializer)?;
    if wire.content.format_version != WORKLOAD_NETWORK_PLAN_FORMAT_VERSION {
        return Err(serde::de::Error::custom(
            WorkloadNetworkPlanError::UnsupportedFormatVersion {
                candidate: wire.content.format_version,
            },
        ));
    }
    let identity = WorkloadNetworkPlanIdentity::new(
        wire.content.identity.tenant_id,
        wire.content.identity.workload_incarnation_key,
        wire.content.identity.generation,
    )
    .map_err(serde::de::Error::custom)?;
    let attachment = wire
        .content
        .attachment
        .map(WorkloadNetworkAttachmentBlueprintWire::into_blueprint)
        .transpose()
        .map_err(serde::de::Error::custom)?;
    let routes = wire
        .content
        .routes
        .into_iter()
        .map(WorkloadNetworkRouteBlueprintWire::into_blueprint)
        .collect::<Result<Vec<_>, _>>()
        .map_err(serde::de::Error::custom)?;
    let listeners = wire
        .content
        .listeners
        .into_iter()
        .map(WorkloadNetworkListenerBlueprintWire::into_blueprint)
        .collect::<Result<Vec<_>, _>>()
        .map_err(serde::de::Error::custom)?;
    let dependency_listeners = wire
        .content
        .dependency_listeners
        .into_iter()
        .map(WorkloadNetworkDependencyListenerBlueprintWire::into_blueprint)
        .collect::<Result<Vec<_>, _>>()
        .map_err(serde::de::Error::custom)?;
    let content = WorkloadNetworkPlanContent::new(
        identity,
        wire.content.capability_requirements,
        wire.content.capability_selection,
        wire.content.capability_selection_evidence,
        attachment,
        routes,
        listeners,
        dependency_listeners,
        wire.content.activation,
        wire.content.publication,
    )
    .map_err(serde::de::Error::custom)?;
    let plan = NetworkPlan::new(
        wire.plan.plan_id,
        wire.plan.generation,
        wire.plan.content_digest,
        wire.plan.requirements,
    )
    .with_readiness_requirements(wire.plan.readiness_requirements)
    .map_err(serde::de::Error::custom)?;
    CompiledWorkloadNetworkPlan::new(plan, content).map_err(serde::de::Error::custom)
}

impl CompiledWorkloadNetworkPlan {
    /// Derive the only valid plan envelope for exact retained content.
    pub fn from_content(
        content: WorkloadNetworkPlanContent,
    ) -> Result<Self, WorkloadNetworkPlanError> {
        let readiness = content.readiness_requirements()?;
        let plan = NetworkPlan::new(
            content.identity.plan_id(),
            content.identity.generation,
            NetworkPlanContentDigest::sha256(content.canonical_bytes()),
            content.capability_requirements.clone(),
        )
        .with_readiness_requirements(readiness)
        .map_err(WorkloadNetworkPlanError::InvalidReadinessRequirements)?;
        Ok(Self { plan, content })
    }

    /// Authenticate every plan-envelope field against retained content.
    pub fn new(
        plan: NetworkPlan,
        content: WorkloadNetworkPlanContent,
    ) -> Result<Self, WorkloadNetworkPlanError> {
        let candidate = NetworkPlanContentDigest::sha256(content.canonical_bytes());
        let expected = plan.content_digest();
        if candidate != expected {
            return Err(WorkloadNetworkPlanError::ContentDigestMismatch {
                expected,
                candidate,
            });
        }
        let expected_plan_id = content.identity.plan_id();
        if plan.plan_id() != &expected_plan_id {
            return Err(WorkloadNetworkPlanError::PlanIdentityMismatch {
                expected: expected_plan_id,
                candidate: plan.plan_id().clone(),
            });
        }
        if plan.generation() != content.identity.generation {
            return Err(WorkloadNetworkPlanError::PlanGenerationMismatch {
                expected: content.identity.generation,
                candidate: plan.generation(),
            });
        }
        if plan.requirements().sovereignty() != content.capability_requirements.sovereignty() {
            return Err(WorkloadNetworkPlanError::PlanSovereigntyMismatch);
        }
        if plan.requirements() != &content.capability_requirements {
            return Err(WorkloadNetworkPlanError::PlanCapabilityRequirementsMismatch);
        }
        if plan.readiness_requirements() != content.readiness_requirements()? {
            return Err(WorkloadNetworkPlanError::PlanReadinessRequirementsMismatch);
        }
        Ok(Self { plan, content })
    }

    /// Desired network-plan envelope.
    pub fn plan(&self) -> &NetworkPlan {
        &self.plan
    }

    /// Exact portable content authenticated by the envelope.
    pub fn content(&self) -> &WorkloadNetworkPlanContent {
        &self.content
    }

    /// Split the authenticated envelope and content without changing either.
    pub fn into_parts(self) -> (NetworkPlan, WorkloadNetworkPlanContent) {
        (self.plan, self.content)
    }
}

impl<'de> Deserialize<'de> for CompiledWorkloadNetworkPlan {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let wire = CompiledWorkloadNetworkPlanWire::deserialize(deserializer)?;
        Self::new(wire.plan, wire.content).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[path = "network_plan/tests.rs"]
mod tests;
