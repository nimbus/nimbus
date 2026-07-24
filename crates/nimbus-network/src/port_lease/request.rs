//! Portable host-port request and overlap vocabulary.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::num::NonZeroU16;

use serde::{Deserialize, Serialize};

const MAX_REALM_ID_LENGTH: usize = 128;

/// Kernel transport whose numeric port namespace is being reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortProtocol {
    /// Transmission Control Protocol.
    Tcp,
    /// User Datagram Protocol.
    Udp,
}

/// Intended reachability of a binding.
///
/// Exposure is policy/admission metadata. It never weakens kernel address
/// overlap: two otherwise-overlapping requests still conflict when their
/// desired exposure differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortExposure {
    /// Reachable only through loopback.
    Loopback,
    /// Reachable through a private network boundary.
    Private,
    /// Reachable through a public network boundary.
    Public,
    /// The provider or host exposure is not yet proven.
    Unknown,
}

/// Validated stable identity of one proven-isolated bind realm.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PortIsolatedRealm(String);

impl PortIsolatedRealm {
    /// Construct a bounded portable realm identity.
    pub fn new(value: impl Into<String>) -> Result<Self, PortBindRealmError> {
        let value = value.into();
        if value.is_empty() {
            return Err(PortBindRealmError {
                kind: PortBindRealmErrorKind::Empty,
            });
        }
        if value.len() > MAX_REALM_ID_LENGTH {
            return Err(PortBindRealmError {
                kind: PortBindRealmErrorKind::TooLong,
            });
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
        {
            return Err(PortBindRealmError {
                kind: PortBindRealmErrorKind::InvalidCharacter,
            });
        }
        Ok(Self(value))
    }

    /// Canonical realm identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PortIsolatedRealm {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Reason a bind-realm identity was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortBindRealmErrorKind {
    /// The identity was empty.
    Empty,
    /// The identity exceeded the portable bound.
    TooLong,
    /// The identity contained a non-portable character.
    InvalidCharacter,
}

/// Invalid isolated bind-realm identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortBindRealmError {
    kind: PortBindRealmErrorKind,
}

impl PortBindRealmError {
    /// Stable rejection reason.
    pub const fn kind(&self) -> PortBindRealmErrorKind {
        self.kind
    }
}

impl Display for PortBindRealmError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            PortBindRealmErrorKind::Empty => "isolated port bind realm must not be empty",
            PortBindRealmErrorKind::TooLong => {
                "isolated port bind realm exceeds 128-byte portable limit"
            }
            PortBindRealmErrorKind::InvalidCharacter => {
                "isolated port bind realm may contain only ASCII letters, digits, '-', '_', '.', or ':'"
            }
        })
    }
}

impl StdError for PortBindRealmError {}

/// Kernel namespace in which a bind occurs.
///
/// `ProvenIsolated` asserts that the effect adapter has proved non-overlap
/// with other realm identities. `Unknown` overlaps every realm and is the
/// safe default when that proof is unavailable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortBindRealm {
    /// Node host network namespace.
    Host,
    /// Provider/host semantics are not known; conflict with every realm.
    Unknown,
    /// A stable, provider-proven isolated namespace.
    ProvenIsolated(PortIsolatedRealm),
}

impl PortBindRealm {
    /// Construct a validated proven-isolated realm.
    pub fn proven_isolated(value: impl Into<String>) -> Result<Self, PortBindRealmError> {
        PortIsolatedRealm::new(value).map(Self::ProvenIsolated)
    }

    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) | (Self::Host, Self::Host) => true,
            (Self::ProvenIsolated(first), Self::ProvenIsolated(second)) => first == second,
            (Self::Host, Self::ProvenIsolated(_)) | (Self::ProvenIsolated(_), Self::Host) => false,
        }
    }

    pub(crate) fn accepts_bound(&self, actual: &Self) -> bool {
        !matches!(actual, Self::Unknown) && (matches!(self, Self::Unknown) || self == actual)
    }
}

/// Whether an IPv6 socket can occupy the IPv4 port namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortIpv6Overlap {
    /// Host/provider behavior is unknown; conservatively overlap IPv4.
    Unknown,
    /// The provider is known to accept IPv4 through the IPv6 binding.
    OverlapsIpv4,
    /// The adapter has proved an IPv6-only binding that is disjoint from IPv4.
    ProvenDisjoint,
}

/// Address family of a known bind target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortAddressFamily {
    /// Internet Protocol version 4.
    Ipv4,
    /// Internet Protocol version 6.
    Ipv6,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PortBindTargetWire {
    Unknown,
    Ipv4Wildcard,
    Ipv4Specific {
        address: Ipv4Addr,
    },
    Ipv6Wildcard {
        ipv4_overlap: PortIpv6Overlap,
    },
    Ipv6Specific {
        address: Ipv6Addr,
        ipv4_overlap: PortIpv6Overlap,
    },
}

/// Portable desired bind address plus conservative family-overlap semantics.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct PortBindTarget(PortBindTargetWire);

impl PortBindTarget {
    /// Unknown provider/host target, conservatively overlapping every address.
    pub const fn unknown() -> Self {
        Self(PortBindTargetWire::Unknown)
    }

    /// IPv4 wildcard target.
    pub const fn ipv4_wildcard() -> Self {
        Self(PortBindTargetWire::Ipv4Wildcard)
    }

    /// One specific IPv4 address.
    pub const fn ipv4_specific(address: Ipv4Addr) -> Self {
        Self(PortBindTargetWire::Ipv4Specific { address })
    }

    /// IPv6 wildcard target with explicit cross-family evidence.
    pub const fn ipv6_wildcard(ipv4_overlap: PortIpv6Overlap) -> Self {
        Self(PortBindTargetWire::Ipv6Wildcard { ipv4_overlap })
    }

    /// One specific IPv6 address with explicit cross-family evidence.
    pub fn ipv6_specific(
        address: Ipv6Addr,
        ipv4_overlap: PortIpv6Overlap,
    ) -> Result<Self, PortBindTargetError> {
        Self::from_wire(PortBindTargetWire::Ipv6Specific {
            address,
            ipv4_overlap,
        })
    }

    /// Known address family, or `None` when target semantics are unknown.
    pub const fn family(&self) -> Option<PortAddressFamily> {
        match self.0 {
            PortBindTargetWire::Unknown => None,
            PortBindTargetWire::Ipv4Wildcard | PortBindTargetWire::Ipv4Specific { .. } => {
                Some(PortAddressFamily::Ipv4)
            }
            PortBindTargetWire::Ipv6Wildcard { .. } | PortBindTargetWire::Ipv6Specific { .. } => {
                Some(PortAddressFamily::Ipv6)
            }
        }
    }

    /// Whether the target is a wildcard for its known address family.
    pub const fn is_wildcard(&self) -> bool {
        matches!(
            self.0,
            PortBindTargetWire::Ipv4Wildcard | PortBindTargetWire::Ipv6Wildcard { .. }
        )
    }

    /// Whether host/provider address semantics are unknown.
    pub const fn is_unknown(&self) -> bool {
        matches!(self.0, PortBindTargetWire::Unknown)
    }

    /// Specific address, when this is not wildcard or unknown.
    pub const fn specific_address(&self) -> Option<IpAddr> {
        match self.0 {
            PortBindTargetWire::Ipv4Specific { address } => Some(IpAddr::V4(address)),
            PortBindTargetWire::Ipv6Specific { address, .. } => Some(IpAddr::V6(address)),
            _ => None,
        }
    }

    /// IPv6-to-IPv4 overlap evidence, when this is an IPv6 target.
    pub const fn ipv6_overlap(&self) -> Option<PortIpv6Overlap> {
        match self.0 {
            PortBindTargetWire::Ipv6Wildcard { ipv4_overlap }
            | PortBindTargetWire::Ipv6Specific { ipv4_overlap, .. } => Some(ipv4_overlap),
            _ => None,
        }
    }

    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        use PortBindTargetWire::{Ipv4Specific, Ipv4Wildcard, Ipv6Specific, Ipv6Wildcard, Unknown};

        match (&self.0, &other.0) {
            (Unknown, _) | (_, Unknown) => true,
            (Ipv4Wildcard, Ipv4Wildcard | Ipv4Specific { .. })
            | (Ipv4Specific { .. }, Ipv4Wildcard) => true,
            (Ipv4Specific { address: first }, Ipv4Specific { address: second }) => first == second,
            (Ipv6Wildcard { .. }, Ipv6Wildcard { .. } | Ipv6Specific { .. })
            | (Ipv6Specific { .. }, Ipv6Wildcard { .. }) => true,
            (
                Ipv6Specific { address: first, .. },
                Ipv6Specific {
                    address: second, ..
                },
            ) => first == second,
            (first, second) => {
                let ipv6_overlap = match (first, second) {
                    (Ipv6Wildcard { ipv4_overlap }, _)
                    | (Ipv6Specific { ipv4_overlap, .. }, _)
                    | (_, Ipv6Wildcard { ipv4_overlap })
                    | (_, Ipv6Specific { ipv4_overlap, .. }) => *ipv4_overlap,
                    _ => return false,
                };
                ipv6_overlap != PortIpv6Overlap::ProvenDisjoint
            }
        }
    }

    pub(crate) fn accepts_bound(&self, actual: &Self) -> bool {
        if actual.is_unknown() {
            return false;
        }
        match (&self.0, &actual.0) {
            (PortBindTargetWire::Unknown, _) => true,
            (PortBindTargetWire::Ipv4Wildcard, PortBindTargetWire::Ipv4Wildcard) => true,
            (
                PortBindTargetWire::Ipv4Specific { address: expected },
                PortBindTargetWire::Ipv4Specific { address: actual },
            ) => expected == actual,
            (
                PortBindTargetWire::Ipv6Wildcard {
                    ipv4_overlap: expected,
                },
                PortBindTargetWire::Ipv6Wildcard {
                    ipv4_overlap: actual,
                },
            ) => ipv6_evidence_accepts(*expected, *actual),
            (
                PortBindTargetWire::Ipv6Specific {
                    address: expected_address,
                    ipv4_overlap: expected_overlap,
                },
                PortBindTargetWire::Ipv6Specific {
                    address: actual_address,
                    ipv4_overlap: actual_overlap,
                },
            ) => {
                expected_address == actual_address
                    && ipv6_evidence_accepts(*expected_overlap, *actual_overlap)
            }
            _ => false,
        }
    }

    fn from_wire(wire: PortBindTargetWire) -> Result<Self, PortBindTargetError> {
        if let PortBindTargetWire::Ipv6Specific { address, .. } = wire
            && address.to_ipv4_mapped().is_some()
        {
            return Err(PortBindTargetError::Ipv4MappedIpv6Address);
        }
        Ok(Self(wire))
    }
}

fn ipv6_evidence_accepts(expected: PortIpv6Overlap, actual: PortIpv6Overlap) -> bool {
    matches!(expected, PortIpv6Overlap::Unknown) || expected == actual
}

impl<'de> Deserialize<'de> for PortBindTarget {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let wire = PortBindTargetWire::deserialize(deserializer)?;
        Self::from_wire(wire).map_err(serde::de::Error::custom)
    }
}

/// Invalid portable bind target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortBindTargetError {
    /// IPv4-mapped IPv6 is ambiguous across the family-overlap boundary.
    Ipv4MappedIpv6Address,
}

impl Display for PortBindTargetError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "IPv4-mapped IPv6 bind targets are not portable; use the canonical IPv4 target",
        )
    }
}

impl StdError for PortBindTargetError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortRangeWire {
    start: NonZeroU16,
    end: NonZeroU16,
}

/// Inclusive non-zero port allocation range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "PortRangeWire", into = "PortRangeWire")]
pub struct PortRange {
    start: NonZeroU16,
    end: NonZeroU16,
}

impl PortRange {
    /// Construct an ordered inclusive range.
    pub fn new(start: NonZeroU16, end: NonZeroU16) -> Result<Self, PortRangeError> {
        if start > end {
            return Err(PortRangeError::StartAfterEnd);
        }
        Ok(Self { start, end })
    }

    /// Inclusive first candidate.
    pub const fn start(&self) -> NonZeroU16 {
        self.start
    }

    /// Inclusive last candidate.
    pub const fn end(&self) -> NonZeroU16 {
        self.end
    }

    pub(crate) fn contains(&self, port: NonZeroU16) -> bool {
        self.start <= port && port <= self.end
    }

    pub(crate) fn candidates(&self) -> impl Iterator<Item = NonZeroU16> {
        (self.start.get()..=self.end.get()).map(|port| {
            NonZeroU16::new(port).expect("validated non-zero range cannot contain port zero")
        })
    }
}

impl TryFrom<PortRangeWire> for PortRange {
    type Error = PortRangeError;

    fn try_from(wire: PortRangeWire) -> Result<Self, Self::Error> {
        Self::new(wire.start, wire.end)
    }
}

impl From<PortRange> for PortRangeWire {
    fn from(range: PortRange) -> Self {
        Self {
            start: range.start,
            end: range.end,
        }
    }
}

/// Invalid inclusive port range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortRangeError {
    /// The first port follows the last port.
    StartAfterEnd,
}

impl Display for PortRangeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("port range start must be less than or equal to its end")
    }
}

impl StdError for PortRangeError {}

/// How a durable lease obtains its numeric port.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortRequestMode {
    /// Reserve this exact port.
    Exact(NonZeroU16),
    /// Atomically select the lowest free port in this inclusive range.
    Range(PortRange),
    /// Let a provider bind port zero and atomically adopt the reported port.
    ProviderAssigned,
}

impl PortRequestMode {
    /// Construct a validated inclusive range request.
    pub fn range(start: NonZeroU16, end: NonZeroU16) -> Result<Self, PortRangeError> {
        PortRange::new(start, end).map(Self::Range)
    }

    pub(crate) fn accepts(&self, port: NonZeroU16) -> bool {
        match self {
            Self::Exact(expected) => *expected == port,
            Self::Range(range) => range.contains(port),
            Self::ProviderAssigned => true,
        }
    }

    pub(crate) fn accepts_attempt(&self, port: u16) -> bool {
        match self {
            Self::Exact(expected) => expected.get() == port,
            Self::Range(range) => NonZeroU16::new(port).is_some_and(|port| range.contains(port)),
            Self::ProviderAssigned => port == 0,
        }
    }
}

/// Portable conflict and allocation domain for one port lease.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortBindingSpec {
    protocol: PortProtocol,
    realm: PortBindRealm,
    target: PortBindTarget,
    exposure: PortExposure,
    port: PortRequestMode,
}

impl PortBindingSpec {
    /// Construct one fully explicit portable binding request.
    pub fn new(
        protocol: PortProtocol,
        realm: PortBindRealm,
        target: PortBindTarget,
        exposure: PortExposure,
        port: PortRequestMode,
    ) -> Self {
        Self {
            protocol,
            realm,
            target,
            exposure,
            port,
        }
    }

    /// TCP or UDP namespace.
    pub const fn protocol(&self) -> PortProtocol {
        self.protocol
    }

    /// Host, unknown, or proven-isolated bind realm.
    pub fn realm(&self) -> &PortBindRealm {
        &self.realm
    }

    /// Desired address and family-overlap evidence.
    pub fn target(&self) -> &PortBindTarget {
        &self.target
    }

    /// Desired reachability metadata.
    pub const fn exposure(&self) -> PortExposure {
        self.exposure
    }

    /// Exact, range, or provider-assigned port request.
    pub fn port(&self) -> &PortRequestMode {
        &self.port
    }

    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        self.protocol == other.protocol
            && self.realm.overlaps(&other.realm)
            && self.target.overlaps(&other.target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realm_overlap_truth_table_is_symmetric_and_conservative() {
        let realms = [
            PortBindRealm::Host,
            PortBindRealm::Unknown,
            PortBindRealm::proven_isolated("realm-a").expect("fixture realm should validate"),
            PortBindRealm::proven_isolated("realm-b").expect("fixture realm should validate"),
        ];

        for (first_index, first) in realms.iter().enumerate() {
            for (second_index, second) in realms.iter().enumerate() {
                let expected = first_index == 1 || second_index == 1 || first_index == second_index;
                assert_eq!(
                    first.overlaps(second),
                    expected,
                    "unexpected realm overlap for {first:?} and {second:?}"
                );
                assert_eq!(first.overlaps(second), second.overlaps(first));
            }
        }
    }

    #[test]
    fn same_family_address_overlap_truth_table_is_complete() {
        let v4 = [
            PortBindTarget::ipv4_wildcard(),
            PortBindTarget::ipv4_specific(Ipv4Addr::new(127, 0, 0, 1)),
            PortBindTarget::ipv4_specific(Ipv4Addr::new(127, 0, 0, 2)),
        ];
        let v6 = [
            PortBindTarget::ipv6_wildcard(PortIpv6Overlap::Unknown),
            PortBindTarget::ipv6_specific(Ipv6Addr::LOCALHOST, PortIpv6Overlap::Unknown)
                .expect("fixture target should validate"),
            PortBindTarget::ipv6_specific(
                "2001:db8::1".parse().expect("fixture address should parse"),
                PortIpv6Overlap::Unknown,
            )
            .expect("fixture target should validate"),
        ];

        for family in [&v4[..], &v6[..]] {
            for (first_index, first) in family.iter().enumerate() {
                for (second_index, second) in family.iter().enumerate() {
                    let expected =
                        first_index == 0 || second_index == 0 || first_index == second_index;
                    assert_eq!(
                        first.overlaps(second),
                        expected,
                        "unexpected address overlap for {first:?} and {second:?}"
                    );
                    assert_eq!(first.overlaps(second), second.overlaps(first));
                }
            }
        }
    }

    #[test]
    fn every_cross_family_pair_obeys_ipv6_evidence_in_both_orders() {
        let v4 = [
            PortBindTarget::ipv4_wildcard(),
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
        ];

        for overlap in [
            PortIpv6Overlap::Unknown,
            PortIpv6Overlap::OverlapsIpv4,
            PortIpv6Overlap::ProvenDisjoint,
        ] {
            let v6 = [
                PortBindTarget::ipv6_wildcard(overlap),
                PortBindTarget::ipv6_specific(Ipv6Addr::LOCALHOST, overlap)
                    .expect("fixture target should validate"),
            ];
            let expected = overlap != PortIpv6Overlap::ProvenDisjoint;

            for first in &v4 {
                for second in &v6 {
                    assert_eq!(first.overlaps(second), expected);
                    assert_eq!(second.overlaps(first), expected);
                }
            }
        }
    }

    #[test]
    fn binding_overlap_cartesian_product_is_symmetric() {
        let realms = [
            PortBindRealm::Host,
            PortBindRealm::Unknown,
            PortBindRealm::proven_isolated("realm-a").expect("fixture realm should validate"),
            PortBindRealm::proven_isolated("realm-b").expect("fixture realm should validate"),
        ];
        let targets = [
            PortBindTarget::unknown(),
            PortBindTarget::ipv4_wildcard(),
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortBindTarget::ipv6_wildcard(PortIpv6Overlap::Unknown),
            PortBindTarget::ipv6_wildcard(PortIpv6Overlap::ProvenDisjoint),
            PortBindTarget::ipv6_specific(Ipv6Addr::LOCALHOST, PortIpv6Overlap::ProvenDisjoint)
                .expect("fixture target should validate"),
        ];
        let mut specs = Vec::new();
        for protocol in [PortProtocol::Tcp, PortProtocol::Udp] {
            for realm in &realms {
                for target in &targets {
                    specs.push(PortBindingSpec::new(
                        protocol,
                        realm.clone(),
                        target.clone(),
                        PortExposure::Unknown,
                        PortRequestMode::Exact(
                            NonZeroU16::new(41_473).expect("fixture port should be non-zero"),
                        ),
                    ));
                }
            }
        }

        assert_eq!(specs.len(), 48);
        for first in &specs {
            assert!(
                first.overlaps(first),
                "overlap must be reflexive: {first:?}"
            );
            for second in &specs {
                assert_eq!(
                    first.overlaps(second),
                    second.overlaps(first),
                    "overlap must be symmetric for {first:?} and {second:?}"
                );
                if first.protocol() != second.protocol() {
                    assert!(!first.overlaps(second));
                }
            }
        }
    }
}
