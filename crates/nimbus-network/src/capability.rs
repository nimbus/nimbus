//! Portable network capability requirements and provider evidence.
//!
//! This module defines closed requirement and evidence dimensions. Exact
//! source-owned role registration and selection live in the sibling capability
//! registry. The test-only aggregate matcher below remains an oracle for the
//! exhaustive dimension matrix; it is not a public selection authority.

use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{PortBindRealm, PortExposure, PortProtocol};

#[cfg(test)]
use crate::NetworkProviderId;

/// Party responsible for realizing a network attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkManagementMode {
    /// Nimbus realizes the attachment through host-owned effects.
    NimbusHostManaged,
    /// The machine or infrastructure provider realizes the attachment.
    ProviderManaged,
}

/// Portable attachment realization shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAttachmentMode {
    /// Workload shares the node host network.
    HostNetwork,
    /// Workload receives a distinct network namespace.
    IsolatedNamespace,
    /// Workload networking is realized inside a virtual-machine guest.
    VirtualMachineGuest,
    /// Infrastructure provider realizes a virtual network attachment.
    ProviderVirtualNetwork,
}

/// Isolation evidence a provider can establish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkIsolationMode {
    /// The workload has a distinct network namespace or equivalent boundary.
    WorkloadNamespace,
    /// The provider establishes a tenant-scoped routed segment.
    TenantSegment,
    /// The infrastructure provider establishes an equivalent isolation boundary.
    ProviderBoundary,
}

/// IP family supported by an attachment or endpoint provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAddressFamily {
    /// Internet Protocol version 4.
    Ipv4,
    /// Internet Protocol version 6.
    Ipv6,
}

/// Proven class of kernel bind realm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkBindRealmKind {
    /// Node host network namespace.
    Host,
    /// Provider-proven isolated namespace.
    ProvenIsolated,
}

/// Proven endpoint exposure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkExposure {
    /// Reachable only through loopback.
    Loopback,
    /// Reachable through a private boundary.
    Private,
    /// Reachable through a public boundary.
    Public,
}

/// How a provider can allocate a numeric port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPortAssignmentMode {
    /// Realize an exact caller-admitted port.
    Exact,
    /// Realize a port selected by Nimbus from an admitted range.
    NimbusAllocatedRange,
    /// Let the effect provider assign the port and report it for adoption.
    ProviderAssigned,
}

/// Ingress behavior a provider can realize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkIngressFeature {
    /// Route by host name.
    HostRouting,
    /// Route by request path.
    PathRouting,
    /// Terminate an admitted TLS identity.
    TlsTermination,
    /// Preserve WebSocket upgrade and framing semantics.
    WebSocket,
    /// Preserve streaming request and response semantics.
    Streaming,
}

/// TLS handling an ingress provider can prove for an admitted endpoint.
///
/// This is capability evidence only. Certificate selection and the concrete
/// TLS effect remain with the ingress owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkTlsBehavior {
    /// Carry cleartext application traffic without TLS handling.
    Disabled,
    /// Preserve TLS bytes for a downstream termination owner.
    Passthrough,
    /// Terminate the admitted TLS identity at the ingress owner.
    TerminateAtIngress,
}

/// Forwarding lifecycle behavior a provider can realize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkForwardingFeature {
    /// Forward a listener to its admitted destination.
    PortForwarding,
    /// Stop new connections and drain existing connections.
    ConnectionDrain,
}

/// Durable provider operation supported for reconciliation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkLifecycleFeature {
    /// Inspect provider state by stable identity and fence.
    DurableInspect,
    /// Reconcile provider state idempotently after restart or ambiguity.
    Reconcile,
    /// Delete provider state and prove absence before authority release.
    Delete,
}

/// Broadest control-plane dependency scope used by a provider.
///
/// Declaration order is the sovereignty order from narrowest to broadest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkControlPlaneLocality {
    /// All control-plane decisions remain on the Nimbus node.
    LocalOnly,
    /// Decisions may use infrastructure controlled by the operator.
    OperatorLocal,
    /// Decisions require a third-party control plane.
    ThirdParty,
}

/// External facility a provider requires to satisfy the reported capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkExternalDependency {
    /// Reachability to the public Internet.
    PublicNetwork,
    /// An external domain-name service.
    Dns,
    /// A hosted certificate service.
    HostedCertificate,
    /// A third-party relay.
    Relay,
    /// An external provider control plane.
    ExternalControlPlane,
}

/// Stable capability dimension used to classify mismatches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkCapabilityDimension {
    /// Attachment management ownership.
    ManagementMode,
    /// Attachment realization shape.
    AttachmentMode,
    /// Isolation proof.
    IsolationMode,
    /// IP address family.
    AddressFamily,
    /// Kernel bind realm.
    BindRealm,
    /// Endpoint exposure.
    Exposure,
    /// Transport protocol.
    Protocol,
    /// Port assignment mode.
    PortAssignment,
    /// Ingress behavior.
    IngressFeature,
    /// TLS handling behavior.
    TlsBehavior,
    /// Forwarding behavior.
    ForwardingFeature,
    /// Durable lifecycle operation.
    LifecycleFeature,
    /// Control-plane locality.
    ControlPlaneLocality,
    /// Required external facility.
    ExternalDependency,
    /// Restart without external connectivity.
    OfflineRestart,
}

macro_rules! impl_stable_display {
    ($type:ty, {$($variant:path => $label:literal),+ $(,)?}) => {
        impl Display for $type {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                formatter.write_str(match self {
                    $($variant => $label,)+
                })
            }
        }
    };
}

impl_stable_display!(NetworkManagementMode, {
    NetworkManagementMode::NimbusHostManaged => "nimbus_host_managed",
    NetworkManagementMode::ProviderManaged => "provider_managed",
});
impl_stable_display!(NetworkAttachmentMode, {
    NetworkAttachmentMode::HostNetwork => "host_network",
    NetworkAttachmentMode::IsolatedNamespace => "isolated_namespace",
    NetworkAttachmentMode::VirtualMachineGuest => "virtual_machine_guest",
    NetworkAttachmentMode::ProviderVirtualNetwork => "provider_virtual_network",
});
impl_stable_display!(NetworkIsolationMode, {
    NetworkIsolationMode::WorkloadNamespace => "workload_namespace",
    NetworkIsolationMode::TenantSegment => "tenant_segment",
    NetworkIsolationMode::ProviderBoundary => "provider_boundary",
});
impl_stable_display!(NetworkAddressFamily, {
    NetworkAddressFamily::Ipv4 => "ipv4",
    NetworkAddressFamily::Ipv6 => "ipv6",
});
impl_stable_display!(NetworkBindRealmKind, {
    NetworkBindRealmKind::Host => "host",
    NetworkBindRealmKind::ProvenIsolated => "proven_isolated",
});
impl_stable_display!(NetworkExposure, {
    NetworkExposure::Loopback => "loopback",
    NetworkExposure::Private => "private",
    NetworkExposure::Public => "public",
});
impl_stable_display!(PortProtocol, {
    PortProtocol::Tcp => "tcp",
    PortProtocol::Udp => "udp",
});
impl_stable_display!(NetworkPortAssignmentMode, {
    NetworkPortAssignmentMode::Exact => "exact",
    NetworkPortAssignmentMode::NimbusAllocatedRange => "nimbus_allocated_range",
    NetworkPortAssignmentMode::ProviderAssigned => "provider_assigned",
});
impl_stable_display!(NetworkIngressFeature, {
    NetworkIngressFeature::HostRouting => "host_routing",
    NetworkIngressFeature::PathRouting => "path_routing",
    NetworkIngressFeature::TlsTermination => "tls_termination",
    NetworkIngressFeature::WebSocket => "web_socket",
    NetworkIngressFeature::Streaming => "streaming",
});
impl_stable_display!(NetworkTlsBehavior, {
    NetworkTlsBehavior::Disabled => "disabled",
    NetworkTlsBehavior::Passthrough => "passthrough",
    NetworkTlsBehavior::TerminateAtIngress => "terminate_at_ingress",
});
impl_stable_display!(NetworkForwardingFeature, {
    NetworkForwardingFeature::PortForwarding => "port_forwarding",
    NetworkForwardingFeature::ConnectionDrain => "connection_drain",
});
impl_stable_display!(NetworkLifecycleFeature, {
    NetworkLifecycleFeature::DurableInspect => "durable_inspect",
    NetworkLifecycleFeature::Reconcile => "reconcile",
    NetworkLifecycleFeature::Delete => "delete",
});
impl_stable_display!(NetworkControlPlaneLocality, {
    NetworkControlPlaneLocality::LocalOnly => "local_only",
    NetworkControlPlaneLocality::OperatorLocal => "operator_local",
    NetworkControlPlaneLocality::ThirdParty => "third_party",
});
impl_stable_display!(NetworkExternalDependency, {
    NetworkExternalDependency::PublicNetwork => "public_network",
    NetworkExternalDependency::Dns => "dns",
    NetworkExternalDependency::HostedCertificate => "hosted_certificate",
    NetworkExternalDependency::Relay => "relay",
    NetworkExternalDependency::ExternalControlPlane => "external_control_plane",
});
impl_stable_display!(NetworkCapabilityDimension, {
    NetworkCapabilityDimension::ManagementMode => "management_mode",
    NetworkCapabilityDimension::AttachmentMode => "attachment_mode",
    NetworkCapabilityDimension::IsolationMode => "isolation_mode",
    NetworkCapabilityDimension::AddressFamily => "address_family",
    NetworkCapabilityDimension::BindRealm => "bind_realm",
    NetworkCapabilityDimension::Exposure => "exposure",
    NetworkCapabilityDimension::Protocol => "protocol",
    NetworkCapabilityDimension::PortAssignment => "port_assignment",
    NetworkCapabilityDimension::IngressFeature => "ingress_feature",
    NetworkCapabilityDimension::TlsBehavior => "tls_behavior",
    NetworkCapabilityDimension::ForwardingFeature => "forwarding_feature",
    NetworkCapabilityDimension::LifecycleFeature => "lifecycle_feature",
    NetworkCapabilityDimension::ControlPlaneLocality => "control_plane_locality",
    NetworkCapabilityDimension::ExternalDependency => "external_dependency",
    NetworkCapabilityDimension::OfflineRestart => "offline_restart",
});

/// A runtime port fact cannot be represented as proven capability evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkCapabilityFactError {
    /// The provider has not established its bind realm.
    UnknownBindRealm,
    /// The provider has not established endpoint exposure.
    UnknownExposure,
}

impl Display for NetworkCapabilityFactError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnknownBindRealm => {
                "unknown bind-realm evidence is not a network capability fact"
            }
            Self::UnknownExposure => "unknown endpoint exposure is not a network capability fact",
        })
    }
}

impl StdError for NetworkCapabilityFactError {}

impl TryFrom<&PortBindRealm> for NetworkBindRealmKind {
    type Error = NetworkCapabilityFactError;

    fn try_from(value: &PortBindRealm) -> Result<Self, Self::Error> {
        match value {
            PortBindRealm::Host => Ok(Self::Host),
            PortBindRealm::ProvenIsolated(_) => Ok(Self::ProvenIsolated),
            PortBindRealm::Unknown => Err(NetworkCapabilityFactError::UnknownBindRealm),
        }
    }
}

impl TryFrom<PortExposure> for NetworkExposure {
    type Error = NetworkCapabilityFactError;

    fn try_from(value: PortExposure) -> Result<Self, Self::Error> {
        match value {
            PortExposure::Loopback => Ok(Self::Loopback),
            PortExposure::Private => Ok(Self::Private),
            PortExposure::Public => Ok(Self::Public),
            PortExposure::Unknown => Err(NetworkCapabilityFactError::UnknownExposure),
        }
    }
}

/// Attachment capability facts interpreted as required or supported by the
/// enclosing value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkAttachmentCapabilitySet {
    management_mode: NetworkManagementMode,
    attachment_modes: BTreeSet<NetworkAttachmentMode>,
    isolation_modes: BTreeSet<NetworkIsolationMode>,
}

impl NetworkAttachmentCapabilitySet {
    /// Construct a fully explicit, canonical attachment fact set.
    pub fn new(
        management_mode: NetworkManagementMode,
        attachment_modes: impl IntoIterator<Item = NetworkAttachmentMode>,
        isolation_modes: impl IntoIterator<Item = NetworkIsolationMode>,
    ) -> Self {
        Self {
            management_mode,
            attachment_modes: attachment_modes.into_iter().collect(),
            isolation_modes: isolation_modes.into_iter().collect(),
        }
    }

    /// Required or offered management ownership.
    pub const fn management_mode(&self) -> NetworkManagementMode {
        self.management_mode
    }

    /// Required or supported attachment shapes.
    pub fn attachment_modes(&self) -> &BTreeSet<NetworkAttachmentMode> {
        &self.attachment_modes
    }

    /// Required or supported isolation proofs.
    pub fn isolation_modes(&self) -> &BTreeSet<NetworkIsolationMode> {
        &self.isolation_modes
    }
}

/// Endpoint capability facts interpreted as required or supported by the
/// enclosing value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkEndpointCapabilitySet {
    address_families: BTreeSet<NetworkAddressFamily>,
    bind_realms: BTreeSet<NetworkBindRealmKind>,
    exposures: BTreeSet<NetworkExposure>,
    protocols: BTreeSet<PortProtocol>,
    port_assignment_modes: BTreeSet<NetworkPortAssignmentMode>,
}

impl NetworkEndpointCapabilitySet {
    /// Construct a fully explicit, canonical endpoint fact set.
    pub fn new(
        address_families: impl IntoIterator<Item = NetworkAddressFamily>,
        bind_realms: impl IntoIterator<Item = NetworkBindRealmKind>,
        exposures: impl IntoIterator<Item = NetworkExposure>,
        protocols: impl IntoIterator<Item = PortProtocol>,
        port_assignment_modes: impl IntoIterator<Item = NetworkPortAssignmentMode>,
    ) -> Self {
        Self {
            address_families: address_families.into_iter().collect(),
            bind_realms: bind_realms.into_iter().collect(),
            exposures: exposures.into_iter().collect(),
            protocols: protocols.into_iter().collect(),
            port_assignment_modes: port_assignment_modes.into_iter().collect(),
        }
    }

    /// Required or supported address families.
    pub fn address_families(&self) -> &BTreeSet<NetworkAddressFamily> {
        &self.address_families
    }

    /// Required or supported proven bind realms.
    pub fn bind_realms(&self) -> &BTreeSet<NetworkBindRealmKind> {
        &self.bind_realms
    }

    /// Required or supported proven exposure classes.
    pub fn exposures(&self) -> &BTreeSet<NetworkExposure> {
        &self.exposures
    }

    /// Required or supported transport protocols.
    pub fn protocols(&self) -> &BTreeSet<PortProtocol> {
        &self.protocols
    }

    /// Required or supported port assignment modes.
    pub fn port_assignment_modes(&self) -> &BTreeSet<NetworkPortAssignmentMode> {
        &self.port_assignment_modes
    }
}

/// Ingress capability facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkIngressCapabilitySet {
    features: BTreeSet<NetworkIngressFeature>,
    tls_behaviors: BTreeSet<NetworkTlsBehavior>,
}

impl NetworkIngressCapabilitySet {
    /// Construct an explicit, canonical ingress fact set.
    pub fn new(features: impl IntoIterator<Item = NetworkIngressFeature>) -> Self {
        Self {
            features: features.into_iter().collect(),
            tls_behaviors: BTreeSet::new(),
        }
    }

    /// Replace the supported TLS behaviors with an explicit canonical set.
    pub fn with_tls_behaviors(
        mut self,
        tls_behaviors: impl IntoIterator<Item = NetworkTlsBehavior>,
    ) -> Self {
        self.tls_behaviors = tls_behaviors.into_iter().collect();
        self
    }

    /// Required or supported ingress features.
    pub fn features(&self) -> &BTreeSet<NetworkIngressFeature> {
        &self.features
    }

    /// Required or supported TLS handling behaviors.
    pub fn tls_behaviors(&self) -> &BTreeSet<NetworkTlsBehavior> {
        &self.tls_behaviors
    }
}

/// Forwarding capability facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkForwardingCapabilitySet {
    features: BTreeSet<NetworkForwardingFeature>,
}

impl NetworkForwardingCapabilitySet {
    /// Construct an explicit, canonical forwarding fact set.
    pub fn new(features: impl IntoIterator<Item = NetworkForwardingFeature>) -> Self {
        Self {
            features: features.into_iter().collect(),
        }
    }

    /// Required or supported forwarding features.
    pub fn features(&self) -> &BTreeSet<NetworkForwardingFeature> {
        &self.features
    }
}

/// Durable lifecycle capability facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkLifecycleCapabilitySet {
    features: BTreeSet<NetworkLifecycleFeature>,
}

impl NetworkLifecycleCapabilitySet {
    /// Construct an explicit, canonical lifecycle fact set.
    pub fn new(features: impl IntoIterator<Item = NetworkLifecycleFeature>) -> Self {
        Self {
            features: features.into_iter().collect(),
        }
    }

    /// Required or supported durable lifecycle features.
    pub fn features(&self) -> &BTreeSet<NetworkLifecycleFeature> {
        &self.features
    }
}

/// Durable lifecycle requirements assigned to each capability role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkLifecycleRequirements {
    attachment: NetworkLifecycleCapabilitySet,
    ingress: NetworkLifecycleCapabilitySet,
}

impl NetworkLifecycleRequirements {
    /// Construct explicit attachment and ingress lifecycle requirements.
    pub fn new(
        attachment: NetworkLifecycleCapabilitySet,
        ingress: NetworkLifecycleCapabilitySet,
    ) -> Self {
        Self {
            attachment,
            ingress,
        }
    }

    /// Lifecycle requirements for the attachment provider.
    pub fn attachment(&self) -> &NetworkLifecycleCapabilitySet {
        &self.attachment
    }

    /// Lifecycle requirements for the ingress provider.
    pub fn ingress(&self) -> &NetworkLifecycleCapabilitySet {
        &self.ingress
    }
}

/// Sovereignty constraints admitted into one network plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkSovereigntyRequirements {
    maximum_control_plane_locality: NetworkControlPlaneLocality,
    allowed_external_dependencies: BTreeSet<NetworkExternalDependency>,
    offline_restart_required: bool,
}

impl NetworkSovereigntyRequirements {
    /// Construct fully explicit sovereignty constraints.
    pub fn new(
        maximum_control_plane_locality: NetworkControlPlaneLocality,
        allowed_external_dependencies: impl IntoIterator<Item = NetworkExternalDependency>,
        offline_restart_required: bool,
    ) -> Self {
        Self {
            maximum_control_plane_locality,
            allowed_external_dependencies: allowed_external_dependencies.into_iter().collect(),
            offline_restart_required,
        }
    }

    /// Broadest admitted control-plane scope.
    pub const fn maximum_control_plane_locality(&self) -> NetworkControlPlaneLocality {
        self.maximum_control_plane_locality
    }

    /// External facilities the admitted plan permits.
    pub fn allowed_external_dependencies(&self) -> &BTreeSet<NetworkExternalDependency> {
        &self.allowed_external_dependencies
    }

    /// Whether restart must succeed without external connectivity.
    pub const fn offline_restart_required(&self) -> bool {
        self.offline_restart_required
    }
}

/// Sovereignty evidence reported by one provider registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkSovereigntyCapabilities {
    control_plane_locality: NetworkControlPlaneLocality,
    required_external_dependencies: BTreeSet<NetworkExternalDependency>,
    offline_restart_supported: bool,
}

impl NetworkSovereigntyCapabilities {
    /// Construct fully explicit provider sovereignty evidence.
    pub fn new(
        control_plane_locality: NetworkControlPlaneLocality,
        required_external_dependencies: impl IntoIterator<Item = NetworkExternalDependency>,
        offline_restart_supported: bool,
    ) -> Self {
        Self {
            control_plane_locality,
            required_external_dependencies: required_external_dependencies.into_iter().collect(),
            offline_restart_supported,
        }
    }

    /// Provider's broadest control-plane dependency scope.
    pub const fn control_plane_locality(&self) -> NetworkControlPlaneLocality {
        self.control_plane_locality
    }

    /// External facilities required by this provider.
    pub fn required_external_dependencies(&self) -> &BTreeSet<NetworkExternalDependency> {
        &self.required_external_dependencies
    }

    /// Whether restart can reconcile without external connectivity.
    pub const fn offline_restart_supported(&self) -> bool {
        self.offline_restart_supported
    }
}

/// Provider-neutral capability requirements carried by desired network state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkCapabilityRequirements {
    attachment: NetworkAttachmentCapabilitySet,
    endpoint: NetworkEndpointCapabilitySet,
    ingress: NetworkIngressCapabilitySet,
    forwarding: NetworkForwardingCapabilitySet,
    lifecycle: NetworkLifecycleRequirements,
    sovereignty: NetworkSovereigntyRequirements,
}

impl NetworkCapabilityRequirements {
    /// Construct a requirement set with every concept group supplied.
    pub fn new(
        attachment: NetworkAttachmentCapabilitySet,
        endpoint: NetworkEndpointCapabilitySet,
        ingress: NetworkIngressCapabilitySet,
        forwarding: NetworkForwardingCapabilitySet,
        lifecycle: NetworkLifecycleRequirements,
        sovereignty: NetworkSovereigntyRequirements,
    ) -> Self {
        Self {
            attachment,
            endpoint,
            ingress,
            forwarding,
            lifecycle,
            sovereignty,
        }
    }

    /// Attachment requirements.
    pub fn attachment(&self) -> &NetworkAttachmentCapabilitySet {
        &self.attachment
    }

    /// Endpoint requirements.
    pub fn endpoint(&self) -> &NetworkEndpointCapabilitySet {
        &self.endpoint
    }

    /// Ingress requirements.
    pub fn ingress(&self) -> &NetworkIngressCapabilitySet {
        &self.ingress
    }

    /// Forwarding requirements.
    pub fn forwarding(&self) -> &NetworkForwardingCapabilitySet {
        &self.forwarding
    }

    /// Durable lifecycle requirements.
    pub fn lifecycle(&self) -> &NetworkLifecycleRequirements {
        &self.lifecycle
    }

    /// Sovereignty requirements.
    pub fn sovereignty(&self) -> &NetworkSovereigntyRequirements {
        &self.sovereignty
    }
}

/// Capability and sovereignty evidence from one explicitly named provider.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkProviderCapabilities {
    provider_id: NetworkProviderId,
    attachment: NetworkAttachmentCapabilitySet,
    endpoint: NetworkEndpointCapabilitySet,
    ingress: NetworkIngressCapabilitySet,
    forwarding: NetworkForwardingCapabilitySet,
    lifecycle: NetworkLifecycleCapabilitySet,
    sovereignty: NetworkSovereigntyCapabilities,
}

#[cfg(test)]
impl NetworkProviderCapabilities {
    /// Construct a report with every capability group supplied.
    pub fn new(
        provider_id: NetworkProviderId,
        attachment: NetworkAttachmentCapabilitySet,
        endpoint: NetworkEndpointCapabilitySet,
        ingress: NetworkIngressCapabilitySet,
        forwarding: NetworkForwardingCapabilitySet,
        lifecycle: NetworkLifecycleCapabilitySet,
        sovereignty: NetworkSovereigntyCapabilities,
    ) -> Self {
        Self {
            provider_id,
            attachment,
            endpoint,
            ingress,
            forwarding,
            lifecycle,
            sovereignty,
        }
    }

    /// Stable registration identity whose evidence is being evaluated.
    pub fn provider_id(&self) -> &NetworkProviderId {
        &self.provider_id
    }

    /// Prove this provider satisfies an admitted requirement set.
    ///
    /// Safe alternatives are diagnostic evidence supplied by the caller. This
    /// method sorts and deduplicates them, but never selects or invokes one.
    pub fn ensure_satisfied(
        &self,
        requirements: &NetworkCapabilityRequirements,
        safe_alternatives: impl IntoIterator<Item = NetworkProviderId>,
    ) -> Result<(), NetworkCapabilitySatisfactionError> {
        let mut mismatches = Vec::new();

        if self.attachment.management_mode != requirements.attachment.management_mode {
            mismatches.push(NetworkCapabilityMismatch::ManagementMode {
                required: requirements.attachment.management_mode,
                offered: self.attachment.management_mode,
            });
        }
        for required in requirements
            .attachment
            .attachment_modes
            .difference(&self.attachment.attachment_modes)
        {
            mismatches.push(NetworkCapabilityMismatch::AttachmentMode {
                required: *required,
            });
        }
        for required in requirements
            .attachment
            .isolation_modes
            .difference(&self.attachment.isolation_modes)
        {
            mismatches.push(NetworkCapabilityMismatch::IsolationMode {
                required: *required,
            });
        }
        for required in requirements
            .endpoint
            .address_families
            .difference(&self.endpoint.address_families)
        {
            mismatches.push(NetworkCapabilityMismatch::AddressFamily {
                required: *required,
            });
        }
        for required in requirements
            .endpoint
            .bind_realms
            .difference(&self.endpoint.bind_realms)
        {
            mismatches.push(NetworkCapabilityMismatch::BindRealm {
                required: *required,
            });
        }
        for required in requirements
            .endpoint
            .exposures
            .difference(&self.endpoint.exposures)
        {
            mismatches.push(NetworkCapabilityMismatch::Exposure {
                required: *required,
            });
        }
        for required in requirements
            .endpoint
            .protocols
            .difference(&self.endpoint.protocols)
        {
            mismatches.push(NetworkCapabilityMismatch::Protocol {
                required: *required,
            });
        }
        for required in requirements
            .endpoint
            .port_assignment_modes
            .difference(&self.endpoint.port_assignment_modes)
        {
            mismatches.push(NetworkCapabilityMismatch::PortAssignment {
                required: *required,
            });
        }
        for required in requirements
            .ingress
            .features
            .difference(&self.ingress.features)
        {
            mismatches.push(NetworkCapabilityMismatch::IngressFeature {
                required: *required,
            });
        }
        for required in requirements
            .ingress
            .tls_behaviors
            .difference(&self.ingress.tls_behaviors)
        {
            mismatches.push(NetworkCapabilityMismatch::TlsBehavior {
                required: *required,
            });
        }
        for required in requirements
            .forwarding
            .features
            .difference(&self.forwarding.features)
        {
            mismatches.push(NetworkCapabilityMismatch::ForwardingFeature {
                required: *required,
            });
        }
        let required_lifecycle: BTreeSet<_> = requirements
            .lifecycle
            .attachment
            .features
            .union(&requirements.lifecycle.ingress.features)
            .copied()
            .collect();
        for required in required_lifecycle.difference(&self.lifecycle.features) {
            mismatches.push(NetworkCapabilityMismatch::LifecycleFeature {
                required: *required,
            });
        }
        if self.sovereignty.control_plane_locality
            > requirements.sovereignty.maximum_control_plane_locality
        {
            mismatches.push(NetworkCapabilityMismatch::ControlPlaneLocality {
                maximum_allowed: requirements.sovereignty.maximum_control_plane_locality,
                offered: self.sovereignty.control_plane_locality,
            });
        }
        for dependency in self
            .sovereignty
            .required_external_dependencies
            .difference(&requirements.sovereignty.allowed_external_dependencies)
        {
            mismatches.push(NetworkCapabilityMismatch::ExternalDependency {
                disallowed: *dependency,
            });
        }
        if requirements.sovereignty.offline_restart_required
            && !self.sovereignty.offline_restart_supported
        {
            mismatches.push(NetworkCapabilityMismatch::OfflineRestart {
                required: true,
                offered: false,
            });
        }

        if mismatches.is_empty() {
            return Ok(());
        }
        let safe_alternatives = safe_alternatives
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Err(NetworkCapabilitySatisfactionError {
            provider_id: self.provider_id.clone(),
            mismatches,
            safe_alternatives,
        })
    }
}

/// One exact provider/requirement mismatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "dimension", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkCapabilityMismatch {
    /// Attachment management ownership differs.
    ManagementMode {
        required: NetworkManagementMode,
        offered: NetworkManagementMode,
    },
    /// A required attachment shape is unsupported.
    AttachmentMode { required: NetworkAttachmentMode },
    /// A required isolation proof is unsupported.
    IsolationMode { required: NetworkIsolationMode },
    /// A required address family is unsupported.
    AddressFamily { required: NetworkAddressFamily },
    /// A required proven bind realm is unsupported.
    BindRealm { required: NetworkBindRealmKind },
    /// A required endpoint exposure is unsupported.
    Exposure { required: NetworkExposure },
    /// A required transport protocol is unsupported.
    Protocol { required: PortProtocol },
    /// A required port assignment mode is unsupported.
    PortAssignment { required: NetworkPortAssignmentMode },
    /// A required ingress behavior is unsupported.
    IngressFeature { required: NetworkIngressFeature },
    /// A required TLS handling behavior is unsupported.
    TlsBehavior { required: NetworkTlsBehavior },
    /// A required forwarding behavior is unsupported.
    ForwardingFeature { required: NetworkForwardingFeature },
    /// A required durable lifecycle operation is unsupported.
    LifecycleFeature { required: NetworkLifecycleFeature },
    /// The provider uses a broader control-plane scope than admitted.
    ControlPlaneLocality {
        maximum_allowed: NetworkControlPlaneLocality,
        offered: NetworkControlPlaneLocality,
    },
    /// The provider requires a facility the plan does not admit.
    ExternalDependency {
        disallowed: NetworkExternalDependency,
    },
    /// Offline restart is required but unsupported.
    OfflineRestart { required: bool, offered: bool },
}

impl NetworkCapabilityMismatch {
    /// Stable dimension of this mismatch.
    pub const fn dimension(&self) -> NetworkCapabilityDimension {
        match self {
            Self::ManagementMode { .. } => NetworkCapabilityDimension::ManagementMode,
            Self::AttachmentMode { .. } => NetworkCapabilityDimension::AttachmentMode,
            Self::IsolationMode { .. } => NetworkCapabilityDimension::IsolationMode,
            Self::AddressFamily { .. } => NetworkCapabilityDimension::AddressFamily,
            Self::BindRealm { .. } => NetworkCapabilityDimension::BindRealm,
            Self::Exposure { .. } => NetworkCapabilityDimension::Exposure,
            Self::Protocol { .. } => NetworkCapabilityDimension::Protocol,
            Self::PortAssignment { .. } => NetworkCapabilityDimension::PortAssignment,
            Self::IngressFeature { .. } => NetworkCapabilityDimension::IngressFeature,
            Self::TlsBehavior { .. } => NetworkCapabilityDimension::TlsBehavior,
            Self::ForwardingFeature { .. } => NetworkCapabilityDimension::ForwardingFeature,
            Self::LifecycleFeature { .. } => NetworkCapabilityDimension::LifecycleFeature,
            Self::ControlPlaneLocality { .. } => NetworkCapabilityDimension::ControlPlaneLocality,
            Self::ExternalDependency { .. } => NetworkCapabilityDimension::ExternalDependency,
            Self::OfflineRestart { .. } => NetworkCapabilityDimension::OfflineRestart,
        }
    }
}

impl Display for NetworkCapabilityMismatch {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManagementMode { required, offered } => write!(
                formatter,
                "{}(required={required}, offered={offered})",
                self.dimension()
            ),
            Self::AttachmentMode { required } => {
                write!(formatter, "{}(required={required})", self.dimension())
            }
            Self::IsolationMode { required } => {
                write!(formatter, "{}(required={required})", self.dimension())
            }
            Self::AddressFamily { required } => {
                write!(formatter, "{}(required={required})", self.dimension())
            }
            Self::BindRealm { required } => {
                write!(formatter, "{}(required={required})", self.dimension())
            }
            Self::Exposure { required } => {
                write!(formatter, "{}(required={required})", self.dimension())
            }
            Self::Protocol { required } => {
                write!(formatter, "{}(required={required})", self.dimension())
            }
            Self::PortAssignment { required } => {
                write!(formatter, "{}(required={required})", self.dimension())
            }
            Self::IngressFeature { required } => {
                write!(formatter, "{}(required={required})", self.dimension())
            }
            Self::TlsBehavior { required } => {
                write!(formatter, "{}(required={required})", self.dimension())
            }
            Self::ForwardingFeature { required } => {
                write!(formatter, "{}(required={required})", self.dimension())
            }
            Self::LifecycleFeature { required } => {
                write!(formatter, "{}(required={required})", self.dimension())
            }
            Self::ControlPlaneLocality {
                maximum_allowed,
                offered,
            } => write!(
                formatter,
                "{}(maximum_allowed={maximum_allowed}, offered={offered})",
                self.dimension()
            ),
            Self::ExternalDependency { disallowed } => {
                write!(formatter, "{}(disallowed={disallowed})", self.dimension())
            }
            Self::OfflineRestart { required, offered } => write!(
                formatter,
                "{}(required={required}, offered={offered})",
                self.dimension()
            ),
        }
    }
}

/// Deterministic rejection of one explicitly named provider.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct NetworkCapabilitySatisfactionError {
    provider_id: NetworkProviderId,
    mismatches: Vec<NetworkCapabilityMismatch>,
    safe_alternatives: Vec<NetworkProviderId>,
}

#[cfg(test)]
impl NetworkCapabilitySatisfactionError {
    /// Provider registration whose evidence was rejected.
    pub fn provider_id(&self) -> &NetworkProviderId {
        &self.provider_id
    }

    /// Mismatches in stable dimension and enum order.
    pub fn mismatches(&self) -> &[NetworkCapabilityMismatch] {
        &self.mismatches
    }

    /// Caller-proven safe alternatives in stable identity order.
    pub fn safe_alternatives(&self) -> &[NetworkProviderId] {
        &self.safe_alternatives
    }
}

#[cfg(test)]
impl Display for NetworkCapabilitySatisfactionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "network provider `{}` does not satisfy requirements: ",
            self.provider_id
        )?;
        for (index, mismatch) in self.mismatches.iter().enumerate() {
            if index != 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{mismatch}")?;
        }
        formatter.write_str("; safe alternatives: ")?;
        if self.safe_alternatives.is_empty() {
            formatter.write_str("none")
        } else {
            for (index, provider_id) in self.safe_alternatives.iter().enumerate() {
                if index != 0 {
                    formatter.write_str(", ")?;
                }
                write!(formatter, "{provider_id}")?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
impl StdError for NetworkCapabilitySatisfactionError {}

#[cfg(test)]
pub(crate) fn test_requirements() -> NetworkCapabilityRequirements {
    test_requirements_with_management(NetworkManagementMode::NimbusHostManaged)
}

#[cfg(test)]
pub(crate) fn test_requirements_with_management(
    management_mode: NetworkManagementMode,
) -> NetworkCapabilityRequirements {
    NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(management_mode, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
        ),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::ThirdParty, [], false),
    )
}

#[cfg(test)]
mod tests;
