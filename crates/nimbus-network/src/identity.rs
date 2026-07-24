use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;

/// The reason a stable network resource ID could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkResourceIdParseErrorKind {
    /// The value belongs to another resource domain or has no domain prefix.
    WrongPrefix,
    /// The payload is not a valid ULID.
    MalformedUlid,
    /// The payload is valid but not in the single canonical wire form.
    NonCanonical,
}

/// Failure to parse a domain-separated stable network resource ID.
///
/// The rejected value is deliberately omitted so diagnostics cannot
/// accidentally copy an operator-supplied identifier into an unbounded or
/// sensitive log field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkResourceIdParseError {
    expected_prefix: &'static str,
    kind: NetworkResourceIdParseErrorKind,
}

impl NetworkResourceIdParseError {
    /// The resource-domain prefix required by the target ID type.
    pub fn expected_prefix(&self) -> &'static str {
        self.expected_prefix
    }

    /// The stable machine-readable reason for rejection.
    pub fn kind(&self) -> NetworkResourceIdParseErrorKind {
        self.kind
    }
}

impl Display for NetworkResourceIdParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let expected = self.expected_prefix;
        match self.kind {
            NetworkResourceIdParseErrorKind::WrongPrefix => {
                write!(formatter, "expected `{expected}_<ULID>`")
            }
            NetworkResourceIdParseErrorKind::MalformedUlid => {
                write!(
                    formatter,
                    "`{expected}` identifier contains an invalid ULID"
                )
            }
            NetworkResourceIdParseErrorKind::NonCanonical => write!(
                formatter,
                "`{expected}` identifier must use the canonical uppercase ULID encoding"
            ),
        }
    }
}

impl StdError for NetworkResourceIdParseError {}

fn generate_stable_id(prefix: &'static str) -> String {
    format!("{prefix}_{}", Ulid::new())
}

fn parse_stable_id(
    value: String,
    expected_prefix: &'static str,
) -> Result<String, NetworkResourceIdParseError> {
    let Some(encoded) = value
        .strip_prefix(expected_prefix)
        .and_then(|remainder| remainder.strip_prefix('_'))
    else {
        return Err(NetworkResourceIdParseError {
            expected_prefix,
            kind: NetworkResourceIdParseErrorKind::WrongPrefix,
        });
    };

    let decoded = Ulid::from_string(encoded).map_err(|_| NetworkResourceIdParseError {
        expected_prefix,
        kind: NetworkResourceIdParseErrorKind::MalformedUlid,
    })?;
    if encoded != decoded.to_string() {
        return Err(NetworkResourceIdParseError {
            expected_prefix,
            kind: NetworkResourceIdParseErrorKind::NonCanonical,
        });
    }

    Ok(value)
}

macro_rules! define_stable_resource_id {
    ($(#[$attribute:meta])* $name:ident, $prefix:literal) => {
        $(#[$attribute])*
        #[derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
        )]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Canonical resource-domain prefix used in text and serialized form.
            pub const PREFIX: &'static str = $prefix;

            /// Generate a new globally unique stable identity.
            pub fn generate() -> Self {
                Self(generate_stable_id(Self::PREFIX))
            }

            /// Return the canonical domain-prefixed representation.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = NetworkResourceIdParseError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.to_owned().try_into()
            }
        }

        impl TryFrom<String> for $name {
            type Error = NetworkResourceIdParseError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                parse_stable_id(value, Self::PREFIX).map(Self)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = NetworkResourceIdParseError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                value.parse()
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
    };
}

define_stable_resource_id!(
    /// Stable identity of compiled provider-neutral connectivity intent.
    NetworkPlanId,
    "netplan"
);
define_stable_resource_id!(
    /// Stable identity of one workload incarnation's named network attachment.
    ///
    /// This is intentionally distinct from a workload ID: one workload may
    /// have multiple attachments, and a replacement workload incarnation must
    /// not inherit a stale attachment identity.
    NetworkAttachmentId,
    "netattachment"
);

impl NetworkAttachmentId {
    /// Derive the stable identity of one named attachment on one workload
    /// incarnation.
    ///
    /// Length-framed, domain-separated hashing makes the result deterministic
    /// across restart while keeping the workload key and attachment name as
    /// separate identity dimensions. Replacing the workload incarnation or
    /// selecting another attachment name necessarily produces a different
    /// identity. The source strings are not retained in the ID.
    pub fn for_workload_attachment(workload_incarnation_key: &str, attachment_name: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"nimbus.network.attachment.v1");
        for component in [workload_incarnation_key, attachment_name] {
            hasher.update(
                u64::try_from(component.len())
                    .expect("a Rust string length always fits u64 on supported targets")
                    .to_be_bytes(),
            );
            hasher.update(component.as_bytes());
        }
        let digest = hasher.finalize();
        let mut payload = [0_u8; 16];
        payload.copy_from_slice(&digest[..16]);
        Self(format!(
            "{}_{}",
            Self::PREFIX,
            Ulid::from(u128::from_be_bytes(payload))
        ))
    }
}

define_stable_resource_id!(
    /// Globally stable portable segment allocation identity.
    ///
    /// It is independent of CIDR, local allocation index, bridge name, and
    /// provider realization.
    NetworkSegmentId,
    "netsegment"
);
define_stable_resource_id!(
    /// Stable identity of a published reachable endpoint.
    PublishedEndpointId,
    "netendpoint"
);
define_stable_resource_id!(
    /// Stable identity of a listener, independent of its observed address.
    ListenerId,
    "netlistener"
);
define_stable_resource_id!(
    /// Stable identity of an admitted ingress route.
    IngressRouteId,
    "netroute"
);
define_stable_resource_id!(
    /// Stable identity of a host-global port reservation.
    PortLeaseId,
    "netportlease"
);
define_stable_resource_id!(
    /// Stable identity of one registered network capability provider.
    NetworkProviderId,
    "netprovider"
);

/// Monotonic desired generation within a [`NetworkPlanId`].
///
/// Generations order desired content and provider observations. Equal
/// generations are idempotent only when their canonical plan digests also
/// match; digest validation belongs to the plan state model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NetworkResourceGeneration(u64);

impl NetworkResourceGeneration {
    /// Construct an explicit generation value.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the underlying monotonic value.
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Advance without allowing an overflow to masquerade as a fresh value.
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

/// Monotonic fencing epoch for one network allocation or lease authority.
///
/// A stale epoch cannot create or publish. Durable handles from an old epoch
/// may still be inspected and cleaned up under the later lifecycle contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NetworkLeaseEpoch(u64);

impl NetworkLeaseEpoch {
    /// Construct an explicit lease-authority epoch.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the underlying monotonic value.
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Advance without allowing an overflow to masquerade as fresh authority.
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use proptest::prelude::*;
    use serde::Serialize;
    use serde::de::DeserializeOwned;

    use super::*;

    trait TestStableId:
        Clone
        + Debug
        + Display
        + FromStr<Err = NetworkResourceIdParseError>
        + Ord
        + Serialize
        + DeserializeOwned
    {
        const PREFIX: &'static str;

        fn as_str(&self) -> &str;
    }

    macro_rules! impl_test_stable_id {
        ($($name:ty),+ $(,)?) => {
            $(
                impl TestStableId for $name {
                    const PREFIX: &'static str = <$name>::PREFIX;

                    fn as_str(&self) -> &str {
                        self.as_str()
                    }
                }
            )+
        };
    }

    impl_test_stable_id!(
        NetworkPlanId,
        NetworkAttachmentId,
        NetworkSegmentId,
        PublishedEndpointId,
        ListenerId,
        IngressRouteId,
        PortLeaseId,
        NetworkProviderId,
    );

    fn canonical_id<T: TestStableId>(raw: u128) -> String {
        format!("{}_{}", T::PREFIX, Ulid::from(raw))
    }

    fn assert_round_trip<T: TestStableId>(raw: u128) -> Result<(), TestCaseError> {
        let encoded = canonical_id::<T>(raw);
        let parsed = encoded
            .parse::<T>()
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        prop_assert_eq!(parsed.as_str(), encoded.as_str());
        prop_assert_eq!(parsed.to_string(), encoded);

        let json = serde_json::to_string(&parsed)
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let decoded: T =
            serde_json::from_str(&json).map_err(|error| TestCaseError::fail(error.to_string()))?;
        prop_assert_eq!(decoded, parsed);
        Ok(())
    }

    fn assert_ordering<T: TestStableId>(left: u128, right: u128) -> Result<(), TestCaseError> {
        let left_id = canonical_id::<T>(left)
            .parse::<T>()
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let right_id = canonical_id::<T>(right)
            .parse::<T>()
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let expected = left.cmp(&right);

        prop_assert_eq!(left_id.cmp(&right_id), expected);
        prop_assert_eq!(left_id.as_str().cmp(right_id.as_str()), expected);
        Ok(())
    }

    fn parse_as<T: TestStableId>(value: &str) -> Result<(), NetworkResourceIdParseError> {
        value.parse::<T>().map(|_| ())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn every_resource_id_round_trips_through_text_and_json(raw in any::<u128>()) {
            assert_round_trip::<NetworkPlanId>(raw)?;
            assert_round_trip::<NetworkAttachmentId>(raw)?;
            assert_round_trip::<NetworkSegmentId>(raw)?;
            assert_round_trip::<PublishedEndpointId>(raw)?;
            assert_round_trip::<ListenerId>(raw)?;
            assert_round_trip::<IngressRouteId>(raw)?;
            assert_round_trip::<PortLeaseId>(raw)?;
            assert_round_trip::<NetworkProviderId>(raw)?;
        }

        #[test]
        fn resource_id_domains_cannot_be_cross_parsed(raw in any::<u128>()) {
            type Parser = fn(&str) -> Result<(), NetworkResourceIdParseError>;
            let parsers: [Parser; 8] = [
                parse_as::<NetworkPlanId>,
                parse_as::<NetworkAttachmentId>,
                parse_as::<NetworkSegmentId>,
                parse_as::<PublishedEndpointId>,
                parse_as::<ListenerId>,
                parse_as::<IngressRouteId>,
                parse_as::<PortLeaseId>,
                parse_as::<NetworkProviderId>,
            ];
            let prefixes = [
                NetworkPlanId::PREFIX,
                NetworkAttachmentId::PREFIX,
                NetworkSegmentId::PREFIX,
                PublishedEndpointId::PREFIX,
                ListenerId::PREFIX,
                IngressRouteId::PREFIX,
                PortLeaseId::PREFIX,
                NetworkProviderId::PREFIX,
            ];
            let payload = Ulid::from(raw);

            for (source_index, source_prefix) in prefixes.iter().enumerate() {
                let encoded = format!("{source_prefix}_{payload}");
                for (target_index, parser) in parsers.iter().enumerate() {
                    let result = parser(&encoded);
                    if source_index == target_index {
                        prop_assert!(result.is_ok());
                    } else {
                        let error = result.expect_err("cross-domain parse must fail");
                        prop_assert_eq!(
                            error.kind(),
                            NetworkResourceIdParseErrorKind::WrongPrefix
                        );
                        prop_assert_eq!(error.expected_prefix(), prefixes[target_index]);
                    }
                }
            }
        }

        #[test]
        fn every_resource_id_preserves_ulid_ordering(
            left in any::<u128>(),
            right in any::<u128>(),
        ) {
            assert_ordering::<NetworkPlanId>(left, right)?;
            assert_ordering::<NetworkAttachmentId>(left, right)?;
            assert_ordering::<NetworkSegmentId>(left, right)?;
            assert_ordering::<PublishedEndpointId>(left, right)?;
            assert_ordering::<ListenerId>(left, right)?;
            assert_ordering::<IngressRouteId>(left, right)?;
            assert_ordering::<PortLeaseId>(left, right)?;
            assert_ordering::<NetworkProviderId>(left, right)?;
        }

        #[test]
        fn generations_and_epochs_round_trip_and_order(
            left in any::<u64>(),
            right in any::<u64>(),
        ) {
            let generation = NetworkResourceGeneration::new(left);
            let generation_json = serde_json::to_string(&generation)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let generation_decoded: NetworkResourceGeneration =
                serde_json::from_str(&generation_json)
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
            prop_assert_eq!(generation_decoded, generation);
            prop_assert_eq!(
                generation.cmp(&NetworkResourceGeneration::new(right)),
                left.cmp(&right)
            );

            let epoch = NetworkLeaseEpoch::new(left);
            let epoch_json = serde_json::to_string(&epoch)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let epoch_decoded: NetworkLeaseEpoch = serde_json::from_str(&epoch_json)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            prop_assert_eq!(epoch_decoded, epoch);
            prop_assert_eq!(
                epoch.cmp(&NetworkLeaseEpoch::new(right)),
                left.cmp(&right)
            );
        }
    }

    #[test]
    fn wire_formats_are_pinned() {
        const PAYLOAD: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

        assert_eq!(
            [
                format!("{}_{PAYLOAD}", NetworkPlanId::PREFIX),
                format!("{}_{PAYLOAD}", NetworkAttachmentId::PREFIX),
                format!("{}_{PAYLOAD}", NetworkSegmentId::PREFIX),
                format!("{}_{PAYLOAD}", PublishedEndpointId::PREFIX),
                format!("{}_{PAYLOAD}", ListenerId::PREFIX),
                format!("{}_{PAYLOAD}", IngressRouteId::PREFIX),
                format!("{}_{PAYLOAD}", PortLeaseId::PREFIX),
                format!("{}_{PAYLOAD}", NetworkProviderId::PREFIX),
            ],
            [
                "netplan_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "netattachment_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "netendpoint_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "netlistener_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "netroute_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "netportlease_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV",
            ]
        );

        let plan = "netplan_01ARZ3NDEKTSV4RRFFQ69G5FAV"
            .parse::<NetworkPlanId>()
            .expect("pinned plan id");
        assert_eq!(
            serde_json::to_string(&plan).expect("serialize plan id"),
            r#""netplan_01ARZ3NDEKTSV4RRFFQ69G5FAV""#
        );
        assert_eq!(
            serde_json::to_string(&NetworkResourceGeneration::new(42))
                .expect("serialize generation"),
            "42"
        );
        assert_eq!(
            serde_json::to_string(&NetworkLeaseEpoch::new(7)).expect("serialize epoch"),
            "7"
        );
    }

    #[test]
    fn generated_ids_use_their_canonical_domain() {
        let generated = NetworkAttachmentId::generate();

        assert!(generated.as_str().starts_with("netattachment_"));
        assert_eq!(generated.as_str().len(), "netattachment_".len() + 26);
        assert_eq!(
            generated.as_str().parse::<NetworkAttachmentId>(),
            Ok(generated)
        );
    }

    #[test]
    fn attachment_identity_is_stable_and_separates_name_and_incarnation() {
        let first =
            NetworkAttachmentId::for_workload_attachment("sandbox-incarnation-a", "default");
        let replay =
            NetworkAttachmentId::for_workload_attachment("sandbox-incarnation-a", "default");
        let another_name =
            NetworkAttachmentId::for_workload_attachment("sandbox-incarnation-a", "service-mesh");
        let replacement =
            NetworkAttachmentId::for_workload_attachment("sandbox-incarnation-b", "default");

        assert_eq!(first, replay);
        assert_ne!(first, another_name);
        assert_ne!(first, replacement);
        assert_eq!(
            first.as_str().parse::<NetworkAttachmentId>(),
            Ok(first.clone())
        );
        assert_ne!(
            NetworkAttachmentId::for_workload_attachment("ab", "c"),
            NetworkAttachmentId::for_workload_attachment("a", "bc"),
            "length framing must prevent component-boundary ambiguity"
        );
    }

    #[test]
    fn serde_rejects_another_resource_domain() {
        let segment = canonical_id::<NetworkSegmentId>(7)
            .parse::<NetworkSegmentId>()
            .expect("canonical segment id");
        let json = serde_json::to_string(&segment).expect("serialize segment id");
        let error = serde_json::from_str::<NetworkPlanId>(&json)
            .expect_err("segment id must not deserialize as a plan id");

        assert!(error.to_string().contains("expected `netplan_<ULID>`"));
    }

    #[test]
    fn malformed_and_noncanonical_ids_fail_with_stable_reasons() {
        let wrong_prefix = "netsegment_00000000000000000000000000"
            .parse::<NetworkPlanId>()
            .expect_err("wrong resource domain must fail");
        assert_eq!(
            wrong_prefix.kind(),
            NetworkResourceIdParseErrorKind::WrongPrefix
        );
        assert_eq!(wrong_prefix.expected_prefix(), NetworkPlanId::PREFIX);

        let malformed = "netplan_not-a-ulid"
            .parse::<NetworkPlanId>()
            .expect_err("malformed payload must fail");
        assert_eq!(
            malformed.kind(),
            NetworkResourceIdParseErrorKind::MalformedUlid
        );

        let noncanonical = "netplan_01arz3ndektsv4rrffq69g5fav"
            .parse::<NetworkPlanId>()
            .expect_err("lowercase payload must fail");
        assert_eq!(
            noncanonical.kind(),
            NetworkResourceIdParseErrorKind::NonCanonical
        );
    }

    #[test]
    fn monotonic_tokens_never_wrap() {
        assert_eq!(
            NetworkResourceGeneration::new(41).checked_next(),
            Some(NetworkResourceGeneration::new(42))
        );
        assert_eq!(
            NetworkResourceGeneration::new(u64::MAX).checked_next(),
            None
        );
        assert_eq!(
            NetworkLeaseEpoch::new(8).checked_next(),
            Some(NetworkLeaseEpoch::new(9))
        );
        assert_eq!(NetworkLeaseEpoch::new(u64::MAX).checked_next(), None);
    }

    #[test]
    fn numeric_token_types_remain_distinct() {
        assert_ne!(
            std::any::TypeId::of::<NetworkResourceGeneration>(),
            std::any::TypeId::of::<NetworkLeaseEpoch>()
        );
    }
}
