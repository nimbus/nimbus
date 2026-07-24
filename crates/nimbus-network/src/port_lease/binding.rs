//! Transport-free provider bind/adoption evidence.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::num::NonZeroU16;

use serde::{Deserialize, Serialize};

use super::{PortBindRealm, PortBindTarget, PortBindingSpec, PortProtocol, PortRequestMode};
use crate::NetworkProviderHandle;

/// Concrete bound endpoint reported by an effect-owning adapter.
///
/// The endpoint cannot contain an unknown realm or target: adoption is the
/// boundary where an adapter must report what the socket actually occupies.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "PortBoundEndpointWire", into = "PortBoundEndpointWire")]
pub struct PortBoundEndpoint {
    protocol: PortProtocol,
    realm: PortBindRealm,
    target: PortBindTarget,
    port: NonZeroU16,
}

impl PortBoundEndpoint {
    /// Construct concrete provider-observed endpoint evidence.
    pub fn new(
        protocol: PortProtocol,
        realm: PortBindRealm,
        target: PortBindTarget,
        port: NonZeroU16,
    ) -> Result<Self, PortBoundEndpointError> {
        if matches!(realm, PortBindRealm::Unknown) {
            return Err(PortBoundEndpointError::UnknownRealm);
        }
        if target.is_unknown() {
            return Err(PortBoundEndpointError::UnknownTarget);
        }
        Ok(Self {
            protocol,
            realm,
            target,
            port,
        })
    }

    /// TCP or UDP namespace observed by the adapter.
    pub const fn protocol(&self) -> PortProtocol {
        self.protocol
    }

    /// Concrete host or proven-isolated bind realm.
    pub fn realm(&self) -> &PortBindRealm {
        &self.realm
    }

    /// Concrete wildcard or specific bind target.
    pub fn target(&self) -> &PortBindTarget {
        &self.target
    }

    /// Actual non-zero bound port.
    pub const fn port(&self) -> NonZeroU16 {
        self.port
    }

    fn mismatch(&self, request: &PortBindingSpec) -> Option<PortBindingMismatch> {
        if request.protocol() != self.protocol {
            return Some(PortBindingMismatch::Protocol);
        }
        if !request.realm().accepts_bound(&self.realm) {
            return Some(PortBindingMismatch::Realm);
        }
        if !request.target().accepts_bound(&self.target) {
            return Some(PortBindingMismatch::Target);
        }
        if !request.port().accepts(self.port) {
            return Some(PortBindingMismatch::Port);
        }
        None
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortBoundEndpointWire {
    protocol: PortProtocol,
    realm: PortBindRealm,
    target: PortBindTarget,
    port: NonZeroU16,
}

impl TryFrom<PortBoundEndpointWire> for PortBoundEndpoint {
    type Error = PortBoundEndpointError;

    fn try_from(wire: PortBoundEndpointWire) -> Result<Self, Self::Error> {
        Self::new(wire.protocol, wire.realm, wire.target, wire.port)
    }
}

impl From<PortBoundEndpoint> for PortBoundEndpointWire {
    fn from(endpoint: PortBoundEndpoint) -> Self {
        Self {
            protocol: endpoint.protocol,
            realm: endpoint.realm,
            target: endpoint.target,
            port: endpoint.port,
        }
    }
}

/// Invalid concrete bind evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortBoundEndpointError {
    /// The effect adapter did not identify the actual bind realm.
    UnknownRealm,
    /// The effect adapter did not identify the actual bind target.
    UnknownTarget,
}

impl Display for PortBoundEndpointError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnknownRealm => "bound port endpoint must have a concrete bind realm",
            Self::UnknownTarget => "bound port endpoint must have a concrete bind target",
        })
    }
}

impl StdError for PortBoundEndpointError {}

/// Concrete bind operation attempted by an effect-owning adapter.
///
/// Unlike a successfully bound endpoint, an attempt may carry port zero when
/// the durable request delegates numeric selection to the provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "PortBindAttemptWire", into = "PortBindAttemptWire")]
pub struct PortBindAttempt {
    protocol: PortProtocol,
    realm: PortBindRealm,
    target: PortBindTarget,
    port: u16,
}

impl PortBindAttempt {
    /// Construct concrete provider-attempt evidence.
    pub fn new(
        protocol: PortProtocol,
        realm: PortBindRealm,
        target: PortBindTarget,
        port: u16,
    ) -> Result<Self, PortBindAttemptError> {
        if matches!(realm, PortBindRealm::Unknown) {
            return Err(PortBindAttemptError::UnknownRealm);
        }
        if target.is_unknown() {
            return Err(PortBindAttemptError::UnknownTarget);
        }
        Ok(Self {
            protocol,
            realm,
            target,
            port,
        })
    }

    /// TCP or UDP namespace used by the attempt.
    pub const fn protocol(&self) -> PortProtocol {
        self.protocol
    }

    /// Concrete host or proven-isolated bind realm used by the attempt.
    pub fn realm(&self) -> &PortBindRealm {
        &self.realm
    }

    /// Concrete wildcard or specific bind target used by the attempt.
    pub fn target(&self) -> &PortBindTarget {
        &self.target
    }

    /// Requested numeric port, including zero for provider assignment.
    pub const fn port(&self) -> u16 {
        self.port
    }

    fn mismatch(&self, request: &PortBindingSpec) -> Option<PortBindingMismatch> {
        if request.protocol() != self.protocol {
            return Some(PortBindingMismatch::Protocol);
        }
        if !request.realm().accepts_bound(&self.realm) {
            return Some(PortBindingMismatch::Realm);
        }
        if !request.target().accepts_bound(&self.target) {
            return Some(PortBindingMismatch::Target);
        }
        if !request.port().accepts_attempt(self.port) {
            return Some(PortBindingMismatch::Port);
        }
        None
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortBindAttemptWire {
    protocol: PortProtocol,
    realm: PortBindRealm,
    target: PortBindTarget,
    port: u16,
}

impl TryFrom<PortBindAttemptWire> for PortBindAttempt {
    type Error = PortBindAttemptError;

    fn try_from(wire: PortBindAttemptWire) -> Result<Self, Self::Error> {
        Self::new(wire.protocol, wire.realm, wire.target, wire.port)
    }
}

impl From<PortBindAttempt> for PortBindAttemptWire {
    fn from(attempt: PortBindAttempt) -> Self {
        Self {
            protocol: attempt.protocol,
            realm: attempt.realm,
            target: attempt.target,
            port: attempt.port,
        }
    }
}

/// Invalid concrete bind-attempt evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortBindAttemptError {
    /// The effect adapter did not identify the attempted bind realm.
    UnknownRealm,
    /// The effect adapter did not identify the attempted bind target.
    UnknownTarget,
}

impl Display for PortBindAttemptError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnknownRealm => "port bind attempt must have a concrete bind realm",
            Self::UnknownTarget => "port bind attempt must have a concrete bind target",
        })
    }
}

impl StdError for PortBindAttemptError {}

/// Ownership source of a concrete adopted socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortBindingProvenance {
    /// Nimbus asked the adapter to bind an exact or allocated port.
    NimbusOwned,
    /// Nimbus asked the provider to assign a port and adopted the result.
    ProviderAssigned,
    /// A systemd, operator, or other external owner supplied the socket.
    ExternallyOwned,
}

impl PortBindingProvenance {
    fn accepts_request(self, request: &PortRequestMode) -> bool {
        matches!(
            (self, request),
            (
                Self::NimbusOwned,
                PortRequestMode::Exact(_) | PortRequestMode::Range(_)
            ) | (Self::ProviderAssigned, PortRequestMode::ProviderAssigned)
                | (Self::ExternallyOwned, PortRequestMode::Exact(_))
        )
    }
}

/// Why concrete provider evidence does not satisfy its durable request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortBindingMismatch {
    /// The adapter reported the wrong transport protocol.
    Protocol,
    /// The adapter reported a realm not admitted by the request.
    Realm,
    /// The adapter reported an address/family target not admitted by the request.
    Target,
    /// The adapter reported a port outside the exact/range request.
    Port,
    /// The ownership provenance is incompatible with the request mode.
    Provenance,
}

impl Display for PortBindingMismatch {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Protocol => "provider protocol does not satisfy the durable request",
            Self::Realm => "provider bind realm does not satisfy the durable request",
            Self::Target => "provider bind target does not satisfy the durable request",
            Self::Port => "provider port does not satisfy the durable request",
            Self::Provenance => "provider binding provenance does not satisfy the request mode",
        })
    }
}

impl StdError for PortBindingMismatch {}

/// Concrete provider binding adopted into a reserved lease before activation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortLeaseBinding {
    endpoint: PortBoundEndpoint,
    provenance: PortBindingProvenance,
    provider_handle: NetworkProviderHandle,
}

impl PortLeaseBinding {
    /// Construct provider binding evidence without performing any effect.
    pub fn new(
        endpoint: PortBoundEndpoint,
        provenance: PortBindingProvenance,
        provider_handle: NetworkProviderHandle,
    ) -> Self {
        Self {
            endpoint,
            provenance,
            provider_handle,
        }
    }

    /// Actual endpoint proven by the provider adapter.
    pub fn endpoint(&self) -> &PortBoundEndpoint {
        &self.endpoint
    }

    /// Actual non-zero host port proven by the provider adapter.
    pub const fn actual_port(&self) -> NonZeroU16 {
        self.endpoint.port
    }

    /// Whether Nimbus, the provider, or an external owner created the socket.
    pub const fn provenance(&self) -> PortBindingProvenance {
        self.provenance
    }

    /// Opaque provider handle used only by the owning adapter.
    pub fn provider_handle(&self) -> &NetworkProviderHandle {
        &self.provider_handle
    }

    pub(crate) fn mismatch(&self, request: &PortBindingSpec) -> Option<PortBindingMismatch> {
        self.endpoint.mismatch(request).or_else(|| {
            (!self.provenance.accepts_request(request.port()))
                .then_some(PortBindingMismatch::Provenance)
        })
    }
}

impl fmt::Debug for PortLeaseBinding {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PortLeaseBinding")
            .field("endpoint", &self.endpoint)
            .field("provenance", &self.provenance)
            .field("provider_handle", &self.provider_handle)
            .finish()
    }
}

/// Stable category of a failed provider bind attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortBindFailureKind {
    /// Another kernel owner already occupies the endpoint.
    AddrInUse,
    /// The operating system or provider denied the operation.
    PermissionDenied,
    /// The requested address is not available on the provider.
    AddressNotAvailable,
    /// The provider does not support the requested bind.
    Unsupported,
    /// The provider could not allocate the required resource.
    ResourceExhausted,
    /// A provider-specific failure not represented above.
    Other,
}

/// Durable evidence that a named provider attempt failed before adoption.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortBindFailure {
    kind: PortBindFailureKind,
    attempt: PortBindAttempt,
    provider_attempt: NetworkProviderHandle,
}

impl PortBindFailure {
    /// Construct a portable failed-bind observation.
    pub fn new(
        kind: PortBindFailureKind,
        attempt: PortBindAttempt,
        provider_attempt: NetworkProviderHandle,
    ) -> Self {
        Self {
            kind,
            attempt,
            provider_attempt,
        }
    }

    /// Stable failure category.
    pub const fn kind(&self) -> PortBindFailureKind {
        self.kind
    }

    /// Concrete operation the provider tried, including port zero when used.
    pub fn attempt(&self) -> &PortBindAttempt {
        &self.attempt
    }

    /// Opaque provider attempt identity for diagnostics/reconciliation.
    pub fn provider_attempt(&self) -> &NetworkProviderHandle {
        &self.provider_attempt
    }

    pub(crate) fn mismatch(&self, request: &PortBindingSpec) -> Option<PortBindingMismatch> {
        self.attempt.mismatch(request)
    }
}

impl fmt::Debug for PortBindFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PortBindFailure")
            .field("kind", &self.kind)
            .field("attempt", &self.attempt)
            .field("provider_attempt", &self.provider_attempt)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use crate::{NetworkProviderId, PortExposure, PortIpv6Overlap, PortRange};

    use super::*;

    const PORT: u16 = 41_473;

    #[test]
    fn concrete_endpoint_wire_rejects_unknown_evidence() {
        assert_eq!(
            PortBoundEndpoint::new(
                PortProtocol::Tcp,
                PortBindRealm::Unknown,
                PortBindTarget::ipv4_wildcard(),
                port(PORT),
            ),
            Err(PortBoundEndpointError::UnknownRealm)
        );
        assert_eq!(
            PortBoundEndpoint::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                PortBindTarget::unknown(),
                port(PORT),
            ),
            Err(PortBoundEndpointError::UnknownTarget)
        );

        let endpoint = host_v4_specific(PORT);
        let wire = serde_json::to_string(&endpoint).expect("endpoint should serialize");
        assert_eq!(
            wire,
            r#"{"protocol":"tcp","realm":"host","target":{"kind":"ipv4_specific","address":"127.0.0.1"},"port":41473}"#
        );
        assert_eq!(
            serde_json::from_str::<PortBoundEndpoint>(&wire).expect("endpoint should deserialize"),
            endpoint
        );

        let unknown_realm = wire.replace(r#""realm":"host""#, r#""realm":"unknown""#);
        assert!(
            serde_json::from_str::<PortBoundEndpoint>(&unknown_realm).is_err(),
            "wire input cannot bypass concrete-realm validation"
        );
        let unknown_target = wire.replace(
            r#""target":{"kind":"ipv4_specific","address":"127.0.0.1"}"#,
            r#""target":{"kind":"unknown"}"#,
        );
        assert!(
            serde_json::from_str::<PortBoundEndpoint>(&unknown_target).is_err(),
            "wire input cannot bypass concrete-target validation"
        );
        let unknown_field = wire.replacen('{', r#"{"unexpected":true,"#, 1);
        assert!(
            serde_json::from_str::<PortBoundEndpoint>(&unknown_field).is_err(),
            "unknown concrete endpoint fields must fail closed"
        );

        assert_eq!(
            PortBindAttempt::new(
                PortProtocol::Tcp,
                PortBindRealm::Unknown,
                PortBindTarget::ipv4_wildcard(),
                PORT,
            ),
            Err(PortBindAttemptError::UnknownRealm)
        );
        assert_eq!(
            PortBindAttempt::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                PortBindTarget::unknown(),
                PORT,
            ),
            Err(PortBindAttemptError::UnknownTarget)
        );
        let attempt_wire = wire.replace(r#""port":41473"#, r#""port":0"#);
        assert!(
            serde_json::from_str::<PortBindAttempt>(&unknown_realm).is_err(),
            "attempt wire cannot bypass concrete-realm validation"
        );
        assert!(
            serde_json::from_str::<PortBindAttempt>(&unknown_target).is_err(),
            "attempt wire cannot bypass concrete-target validation"
        );
        assert!(
            serde_json::from_str::<PortBindAttempt>(&attempt_wire.replacen(
                '{',
                r#"{"unexpected":true,"#,
                1
            ))
            .is_err(),
            "unknown bind-attempt fields must fail closed"
        );
    }

    #[test]
    fn endpoint_satisfaction_names_every_mismatch_dimension() {
        let exact = binding_spec(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortRequestMode::Exact(port(PORT)),
        );
        let correct = host_v4_specific(PORT);
        assert_eq!(correct.mismatch(&exact), None);

        assert_eq!(
            PortBoundEndpoint::new(
                PortProtocol::Udp,
                PortBindRealm::Host,
                PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
                port(PORT),
            )
            .expect("UDP endpoint should validate")
            .mismatch(&exact),
            Some(PortBindingMismatch::Protocol)
        );
        assert_eq!(
            PortBoundEndpoint::new(
                PortProtocol::Tcp,
                PortBindRealm::proven_isolated("realm-a").expect("fixture realm should validate"),
                PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
                port(PORT),
            )
            .expect("isolated endpoint should validate")
            .mismatch(&exact),
            Some(PortBindingMismatch::Realm)
        );
        assert_eq!(
            PortBoundEndpoint::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                PortBindTarget::ipv4_wildcard(),
                port(PORT),
            )
            .expect("wildcard endpoint should validate")
            .mismatch(&exact),
            Some(PortBindingMismatch::Target)
        );
        assert_eq!(
            host_v4_specific(PORT + 1).mismatch(&exact),
            Some(PortBindingMismatch::Port)
        );
    }

    #[test]
    fn unknown_desire_may_be_refined_but_positive_evidence_cannot_be_weakened() {
        let unknown = binding_spec(
            PortProtocol::Tcp,
            PortBindRealm::Unknown,
            PortBindTarget::unknown(),
            PortRequestMode::Exact(port(PORT)),
        );
        assert_eq!(host_v4_specific(PORT).mismatch(&unknown), None);

        let desired_v6_only = binding_spec(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv6_specific(Ipv6Addr::LOCALHOST, PortIpv6Overlap::ProvenDisjoint)
                .expect("fixture IPv6 target should validate"),
            PortRequestMode::Exact(port(PORT)),
        );
        let unknown_dual_stack = PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv6_specific(Ipv6Addr::LOCALHOST, PortIpv6Overlap::Unknown)
                .expect("fixture IPv6 target should validate"),
            port(PORT),
        )
        .expect("fixture endpoint should validate");
        assert_eq!(
            unknown_dual_stack.mismatch(&desired_v6_only),
            Some(PortBindingMismatch::Target),
            "actual unknown behavior cannot satisfy desired proven IPv6 isolation"
        );

        let desired_unknown_dual_stack = binding_spec(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv6_specific(Ipv6Addr::LOCALHOST, PortIpv6Overlap::Unknown)
                .expect("fixture IPv6 target should validate"),
            PortRequestMode::Exact(port(PORT)),
        );
        let proven_v6_only = PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv6_specific(Ipv6Addr::LOCALHOST, PortIpv6Overlap::ProvenDisjoint)
                .expect("fixture IPv6 target should validate"),
            port(PORT),
        )
        .expect("fixture endpoint should validate");
        assert_eq!(proven_v6_only.mismatch(&desired_unknown_dual_stack), None);
    }

    #[test]
    fn failed_attempt_preserves_provider_assigned_port_zero() {
        let provider_assigned = binding_spec(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortRequestMode::ProviderAssigned,
        );
        let attempt = PortBindAttempt::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            0,
        )
        .expect("provider-assigned attempt should validate");
        assert_eq!(attempt.mismatch(&provider_assigned), None);
        assert_eq!(
            serde_json::to_string(&attempt).expect("attempt should serialize"),
            r#"{"protocol":"tcp","realm":"host","target":{"kind":"ipv4_specific","address":"127.0.0.1"},"port":0}"#
        );

        let incorrect_nonzero = PortBindAttempt::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PORT,
        )
        .expect("non-zero attempt should validate structurally");
        assert_eq!(
            incorrect_nonzero.mismatch(&provider_assigned),
            Some(PortBindingMismatch::Port)
        );
    }

    #[test]
    fn provenance_is_fenced_to_the_request_mode() {
        let endpoint = host_v4_specific(PORT);
        let exact = binding_spec(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortRequestMode::Exact(port(PORT)),
        );
        let range = binding_spec(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortRequestMode::Range(
                PortRange::new(port(PORT), port(PORT + 1)).expect("fixture range should validate"),
            ),
        );
        let provider_assigned = binding_spec(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortRequestMode::ProviderAssigned,
        );

        for (provenance, request) in [
            (PortBindingProvenance::NimbusOwned, &exact),
            (PortBindingProvenance::NimbusOwned, &range),
            (PortBindingProvenance::ProviderAssigned, &provider_assigned),
            (PortBindingProvenance::ExternallyOwned, &exact),
        ] {
            let binding =
                PortLeaseBinding::new(endpoint.clone(), provenance, provider_handle("valid"));
            assert_eq!(binding.mismatch(request), None);
        }

        for (provenance, request) in [
            (PortBindingProvenance::ProviderAssigned, &exact),
            (PortBindingProvenance::NimbusOwned, &provider_assigned),
            (PortBindingProvenance::ExternallyOwned, &range),
        ] {
            let binding =
                PortLeaseBinding::new(endpoint.clone(), provenance, provider_handle("invalid"));
            assert_eq!(
                binding.mismatch(request),
                Some(PortBindingMismatch::Provenance)
            );
        }
    }

    #[test]
    fn binding_and_failure_diagnostics_redact_provider_attempts() {
        let secret = "provider-secret-attempt";
        let endpoint = host_v4_specific(PORT);
        let binding = PortLeaseBinding::new(
            endpoint.clone(),
            PortBindingProvenance::NimbusOwned,
            provider_handle(secret),
        );
        let failure = PortBindFailure::new(
            PortBindFailureKind::AddrInUse,
            PortBindAttempt::new(
                endpoint.protocol(),
                endpoint.realm().clone(),
                endpoint.target().clone(),
                endpoint.port().get(),
            )
            .expect("fixture attempt should validate"),
            provider_handle(secret),
        );

        assert!(!format!("{binding:?}").contains(secret));
        assert!(!format!("{failure:?}").contains(secret));
        assert_eq!(failure.kind(), PortBindFailureKind::AddrInUse);
        assert_eq!(
            failure.provider_attempt().expose_to_provider(),
            secret,
            "only the explicit provider accessor reveals the attempt identity"
        );
    }

    fn binding_spec(
        protocol: PortProtocol,
        realm: PortBindRealm,
        target: PortBindTarget,
        mode: PortRequestMode,
    ) -> PortBindingSpec {
        PortBindingSpec::new(protocol, realm, target, PortExposure::Unknown, mode)
    }

    fn host_v4_specific(value: u16) -> PortBoundEndpoint {
        PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            port(value),
        )
        .expect("fixture endpoint should validate")
    }

    fn provider_handle(value: &str) -> NetworkProviderHandle {
        let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
            .parse()
            .expect("fixture provider ID should parse");
        NetworkProviderHandle::new(provider_id, value)
            .expect("fixture provider handle should validate")
    }

    fn port(value: u16) -> NonZeroU16 {
        NonZeroU16::new(value).expect("fixture port should be non-zero")
    }
}
