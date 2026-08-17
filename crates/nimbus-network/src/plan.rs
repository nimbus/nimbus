use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::{
    NetworkCapabilityRequirements, NetworkPlanId, NetworkReadinessRequirement,
    NetworkReadinessRequirementError, NetworkResourceGeneration,
};

const PLAN_DIGEST_DOMAIN: &[u8] = b"nimbus.network.plan.digest.v2\0";

/// SHA-256 digest of the upper-layer canonical resource-plan encoding.
///
/// This type is deliberately distinct from [`NetworkPlanDigest`]. A caller
/// supplies only this content digest; [`NetworkPlan`] binds it to canonical
/// capability requirements before exposing the final plan digest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NetworkPlanContentDigest([u8; 32]);

impl NetworkPlanContentDigest {
    /// Digest the upper-layer canonical resource-plan encoding with SHA-256.
    pub fn sha256(canonical_plan: impl AsRef<[u8]>) -> Self {
        Self(Sha256::digest(canonical_plan.as_ref()).into())
    }

    /// Construct from an already verified content SHA-256 value.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Return the raw content SHA-256 value.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Canonical SHA-256 digest of all provider-neutral desired plan content.
///
/// The digest is domain-separated and binds the upper-layer content digest to
/// the canonical serialized capability requirements. It cannot be supplied to
/// a [`NetworkPlan`] independently of those requirements.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NetworkPlanDigest([u8; 32]);

impl NetworkPlanDigest {
    /// Bind one canonical content digest to canonical capability requirements.
    pub fn for_content(
        content_digest: NetworkPlanContentDigest,
        requirements: &NetworkCapabilityRequirements,
        readiness_requirements: &[NetworkReadinessRequirement],
    ) -> Self {
        // These closed value objects contain only enums, booleans, structs, and
        // BTreeSets. Their serde field order and set order are deterministic,
        // and the pinned digest test detects any future wire change.
        let requirements = serde_json::to_vec(requirements)
            .expect("closed network capability requirements always serialize");
        let requirements_len = u64::try_from(requirements.len())
            .expect("a serialized Rust value length fits u64 on supported targets");
        let readiness_requirements = serde_json::to_vec(readiness_requirements)
            .expect("closed network readiness requirements always serialize");
        let readiness_requirements_len = u64::try_from(readiness_requirements.len())
            .expect("a serialized Rust value length fits u64 on supported targets");
        let mut digest = Sha256::new();
        digest.update(PLAN_DIGEST_DOMAIN);
        digest.update(content_digest.as_bytes());
        digest.update(requirements_len.to_be_bytes());
        digest.update(requirements);
        digest.update(readiness_requirements_len.to_be_bytes());
        digest.update(readiness_requirements);
        Self(digest.finalize().into())
    }

    /// Construct from an already verified complete-plan SHA-256 value.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Return the raw complete-plan SHA-256 value.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

fn parse_digest(value: &str) -> Result<[u8; 32], NetworkPlanDigestParseError> {
    if value.len() != 64 {
        return Err(NetworkPlanDigestParseError::WrongLength);
    }
    if value
        .bytes()
        .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
    {
        return Err(NetworkPlanDigestParseError::NonCanonicalHex);
    }

    let mut bytes = [0_u8; 32];
    for (index, output) in bytes.iter_mut().enumerate() {
        let offset = index * 2;
        *output = u8::from_str_radix(&value[offset..offset + 2], 16)
            .map_err(|_| NetworkPlanDigestParseError::NonCanonicalHex)?;
    }
    Ok(bytes)
}

macro_rules! impl_digest_wire {
    ($type:ident) => {
        impl Display for $type {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                for byte in self.0 {
                    write!(formatter, "{byte:02x}")?;
                }
                Ok(())
            }
        }

        impl fmt::Debug for $type {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_tuple(stringify!($type))
                    .field(&self.to_string())
                    .finish()
            }
        }

        impl FromStr for $type {
            type Err = NetworkPlanDigestParseError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                parse_digest(value).map(Self)
            }
        }

        impl Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}

impl_digest_wire!(NetworkPlanContentDigest);
impl_digest_wire!(NetworkPlanDigest);

/// Stable reason a serialized plan or content digest was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPlanDigestParseError {
    /// SHA-256 text must contain exactly 64 characters.
    WrongLength,
    /// Only lowercase hexadecimal is canonical.
    NonCanonicalHex,
}

impl Display for NetworkPlanDigestParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength => {
                formatter.write_str("network plan digest must be 64 lowercase hex characters")
            }
            Self::NonCanonicalHex => {
                formatter.write_str("network plan digest must use canonical lowercase hex")
            }
        }
    }
}

impl StdError for NetworkPlanDigestParseError {}

/// Desired-generation envelope for compiled provider-neutral connectivity.
///
/// Resource content is compiled above this low-dependency contract. Its
/// distinct content digest and typed capability requirements are the complete
/// desired state from which [`Self::digest`] is derived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "NetworkPlanWire")]
pub struct NetworkPlan {
    plan_id: NetworkPlanId,
    generation: NetworkResourceGeneration,
    content_digest: NetworkPlanContentDigest,
    requirements: NetworkCapabilityRequirements,
    readiness_requirements: Vec<NetworkReadinessRequirement>,
}

impl NetworkPlan {
    /// Construct a desired plan envelope from its stable identity, monotonic
    /// generation, canonical resource-content digest, and admitted capability
    /// requirements.
    pub fn new(
        plan_id: NetworkPlanId,
        generation: NetworkResourceGeneration,
        content_digest: NetworkPlanContentDigest,
        requirements: NetworkCapabilityRequirements,
    ) -> Self {
        Self {
            plan_id,
            generation,
            content_digest,
            requirements,
            readiness_requirements: Vec::new(),
        }
    }

    /// Add the complete canonical desired readiness requirement set.
    ///
    /// Input ordering is normalized; exact duplicates are rejected instead of
    /// being silently collapsed.
    pub fn with_readiness_requirements(
        mut self,
        requirements: impl IntoIterator<Item = NetworkReadinessRequirement>,
    ) -> Result<Self, NetworkReadinessRequirementError> {
        self.readiness_requirements = crate::readiness::canonicalize_requirements(requirements)?;
        Ok(self)
    }

    /// Stable identity of this connectivity plan across generations.
    pub fn plan_id(&self) -> &NetworkPlanId {
        &self.plan_id
    }

    /// Monotonic desired generation.
    pub fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }

    /// Upper-layer canonical resource-content digest.
    pub const fn content_digest(&self) -> NetworkPlanContentDigest {
        self.content_digest
    }

    /// Domain-separated digest of resource content plus requirements.
    pub fn digest(&self) -> NetworkPlanDigest {
        NetworkPlanDigest::for_content(
            self.content_digest,
            &self.requirements,
            &self.readiness_requirements,
        )
    }

    /// Provider-neutral capabilities required by this desired generation.
    pub fn requirements(&self) -> &NetworkCapabilityRequirements {
        &self.requirements
    }

    /// Canonically ordered desired readiness requirements.
    pub fn readiness_requirements(&self) -> &[NetworkReadinessRequirement] {
        &self.readiness_requirements
    }

    /// Decide whether another desired envelope is an idempotent replay or a
    /// valid monotonic advance.
    pub fn classify_update(
        &self,
        candidate: &Self,
    ) -> Result<NetworkPlanUpdate, NetworkPlanUpdateError> {
        if candidate.plan_id != self.plan_id {
            return Err(NetworkPlanUpdateError::PlanIdentityMismatch);
        }
        match candidate.generation.cmp(&self.generation) {
            std::cmp::Ordering::Less => Err(NetworkPlanUpdateError::StaleGeneration {
                current: self.generation,
                candidate: candidate.generation,
            }),
            std::cmp::Ordering::Equal
                if candidate.content_digest != self.content_digest
                    || candidate.requirements != self.requirements
                    || candidate.readiness_requirements != self.readiness_requirements =>
            {
                Err(NetworkPlanUpdateError::EqualGenerationContentConflict {
                    generation: self.generation,
                })
            }
            std::cmp::Ordering::Equal => Ok(NetworkPlanUpdate::Idempotent),
            std::cmp::Ordering::Greater => Ok(NetworkPlanUpdate::Advance),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkPlanWire {
    plan_id: NetworkPlanId,
    generation: NetworkResourceGeneration,
    content_digest: NetworkPlanContentDigest,
    requirements: NetworkCapabilityRequirements,
    readiness_requirements: Vec<NetworkReadinessRequirement>,
}

impl TryFrom<NetworkPlanWire> for NetworkPlan {
    type Error = NetworkReadinessRequirementError;

    fn try_from(wire: NetworkPlanWire) -> Result<Self, Self::Error> {
        NetworkPlan::new(
            wire.plan_id,
            wire.generation,
            wire.content_digest,
            wire.requirements,
        )
        .with_readiness_requirements(wire.readiness_requirements)
    }
}

/// Accepted relationship between a current and candidate desired plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPlanUpdate {
    /// Same identity, generation, digest, and capability requirements.
    Idempotent,
    /// Same identity with a strictly newer generation.
    Advance,
}

/// Rejection from desired-plan generation and content validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPlanUpdateError {
    /// The candidate belongs to another stable plan.
    PlanIdentityMismatch,
    /// The candidate generation is older than desired state.
    StaleGeneration {
        current: NetworkResourceGeneration,
        candidate: NetworkResourceGeneration,
    },
    /// Equal generations carry different desired content.
    EqualGenerationContentConflict {
        generation: NetworkResourceGeneration,
    },
}

impl Display for NetworkPlanUpdateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanIdentityMismatch => {
                formatter.write_str("candidate belongs to a different network plan")
            }
            Self::StaleGeneration { current, candidate } => write!(
                formatter,
                "stale network plan generation {}; current generation is {}",
                candidate.as_u64(),
                current.as_u64()
            ),
            Self::EqualGenerationContentConflict { generation } => write!(
                formatter,
                "network plan generation {} has conflicting desired content",
                generation.as_u64()
            ),
        }
    }
}

impl StdError for NetworkPlanUpdateError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NetworkManagementMode;
    use crate::capability::{test_requirements, test_requirements_with_management};

    fn plan_id(value: &str) -> NetworkPlanId {
        value.parse().expect("fixture plan id should parse")
    }

    fn plan(generation: u64, content: &[u8]) -> NetworkPlan {
        NetworkPlan::new(
            plan_id("netplan_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            NetworkResourceGeneration::new(generation),
            NetworkPlanContentDigest::sha256(content),
            test_requirements(),
        )
    }

    #[test]
    fn sha256_and_wire_form_are_pinned() {
        let content_digest = NetworkPlanContentDigest::sha256(b"nimbus-network-plan-v1");

        assert_eq!(
            content_digest.to_string(),
            "818c3d26850ef542623be2fdabbc9ddc4989d58117d08d0506f67b2c6e4757c5"
        );
        assert_eq!(
            serde_json::to_string(&content_digest).expect("content digest should serialize"),
            r#""818c3d26850ef542623be2fdabbc9ddc4989d58117d08d0506f67b2c6e4757c5""#
        );
        assert_eq!(
            serde_json::from_str::<NetworkPlanContentDigest>(
                r#""818c3d26850ef542623be2fdabbc9ddc4989d58117d08d0506f67b2c6e4757c5""#
            )
            .expect("content digest should deserialize"),
            content_digest
        );

        let plan_digest = NetworkPlanDigest::for_content(content_digest, &test_requirements(), &[]);
        assert_eq!(
            plan_digest.to_string(),
            "e3715319a2ace5cae060692e54100ffec523a36eb74469bae1d9d861f57e4bab"
        );
        assert_eq!(
            serde_json::from_str::<NetworkPlanDigest>(
                &serde_json::to_string(&plan_digest).expect("plan digest should serialize")
            )
            .expect("plan digest should deserialize"),
            plan_digest
        );
    }

    #[test]
    fn digest_parser_rejects_length_uppercase_and_non_hex() {
        assert_eq!(
            "00".parse::<NetworkPlanDigest>(),
            Err(NetworkPlanDigestParseError::WrongLength)
        );
        assert_eq!(
            "AA291AA4A88B95F9965FA3BFBA9CBD0325F71F19177D1136FAFDBBE31E3C4345"
                .parse::<NetworkPlanDigest>(),
            Err(NetworkPlanDigestParseError::NonCanonicalHex)
        );
        assert_eq!(
            "ga291aa4a88b95f9965fa3bfba9cbd0325f71f19177d1136fafdbbe31e3c4345"
                .parse::<NetworkPlanDigest>(),
            Err(NetworkPlanDigestParseError::NonCanonicalHex)
        );
    }

    #[test]
    fn desired_update_rules_reject_stale_and_equal_generation_divergence() {
        let current = plan(7, b"desired-a");

        assert_eq!(
            current.classify_update(&plan(7, b"desired-a")),
            Ok(NetworkPlanUpdate::Idempotent)
        );
        assert_eq!(
            current.classify_update(&plan(8, b"desired-b")),
            Ok(NetworkPlanUpdate::Advance)
        );
        assert_eq!(
            current.classify_update(&plan(6, b"desired-a")),
            Err(NetworkPlanUpdateError::StaleGeneration {
                current: NetworkResourceGeneration::new(7),
                candidate: NetworkResourceGeneration::new(6),
            })
        );
        assert_eq!(
            current.classify_update(&plan(7, b"desired-b")),
            Err(NetworkPlanUpdateError::EqualGenerationContentConflict {
                generation: NetworkResourceGeneration::new(7),
            })
        );
    }

    #[test]
    fn desired_update_rejects_another_plan_identity() {
        let current = plan(7, b"desired");
        let candidate = NetworkPlan::new(
            plan_id("netplan_01ARZ3NDEKTSV4RRFFQ69G5FAW"),
            NetworkResourceGeneration::new(8),
            NetworkPlanContentDigest::sha256(b"desired"),
            test_requirements(),
        );

        assert_eq!(
            current.classify_update(&candidate),
            Err(NetworkPlanUpdateError::PlanIdentityMismatch)
        );
    }

    #[test]
    fn desired_update_rejects_equal_generation_requirement_divergence() {
        let current = plan(7, b"desired");
        let candidate = NetworkPlan::new(
            current.plan_id().clone(),
            current.generation(),
            current.content_digest(),
            test_requirements_with_management(NetworkManagementMode::ProviderManaged),
        );

        assert_eq!(
            current.classify_update(&candidate),
            Err(NetworkPlanUpdateError::EqualGenerationContentConflict {
                generation: NetworkResourceGeneration::new(7),
            })
        );
    }

    #[test]
    fn capability_requirements_are_bound_into_the_plan_digest() {
        let current = plan(7, b"desired");
        let changed_requirements = NetworkPlan::new(
            current.plan_id().clone(),
            NetworkResourceGeneration::new(8),
            current.content_digest(),
            test_requirements_with_management(NetworkManagementMode::ProviderManaged),
        );

        assert_ne!(
            changed_requirements.digest(),
            current.digest(),
            "changing only capability requirements must change the canonical plan digest"
        );
    }

    #[test]
    fn plan_wire_requires_requirements_and_rejects_unknown_fields() {
        let plan = plan(7, b"desired");
        let wire = serde_json::to_value(&plan).expect("network plan should serialize");

        assert_eq!(
            serde_json::from_value::<NetworkPlan>(wire.clone())
                .expect("network plan should round-trip"),
            plan
        );
        assert_eq!(
            wire.get("requirements"),
            Some(
                &serde_json::to_value(test_requirements())
                    .expect("requirements fixture should serialize")
            )
        );
        assert_eq!(
            wire.get("readiness_requirements"),
            Some(&serde_json::json!([])),
            "empty readiness remains explicit desired state on the wire"
        );
        assert_eq!(
            wire.get("content_digest"),
            Some(
                &serde_json::to_value(plan.content_digest())
                    .expect("content digest should serialize")
            )
        );
        assert!(
            wire.get("digest").is_none(),
            "the complete plan digest is derived and cannot diverge on the wire"
        );

        let mut missing_requirements = wire.clone();
        missing_requirements
            .as_object_mut()
            .expect("plan wire should be an object")
            .remove("requirements");
        assert!(
            serde_json::from_value::<NetworkPlan>(missing_requirements).is_err(),
            "requirements are desired state and must never default"
        );

        let mut missing_readiness = wire.clone();
        missing_readiness
            .as_object_mut()
            .expect("plan wire should be an object")
            .remove("readiness_requirements");
        assert!(
            serde_json::from_value::<NetworkPlan>(missing_readiness).is_err(),
            "readiness requirements are desired state and must never default"
        );

        let mut supplied_final_digest = wire;
        supplied_final_digest
            .as_object_mut()
            .expect("plan wire should be an object")
            .insert(
                "digest".to_owned(),
                serde_json::to_value(plan.digest()).expect("plan digest should serialize"),
            );
        assert!(
            serde_json::from_value::<NetworkPlan>(supplied_final_digest).is_err(),
            "wire callers cannot supply a final digest independently"
        );
    }
}
