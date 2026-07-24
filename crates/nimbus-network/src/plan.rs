use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::{NetworkPlanId, NetworkResourceGeneration};

/// Canonical SHA-256 digest of one compiled, provider-neutral plan generation.
///
/// The compiler above this crate owns canonical encoding of admitted intent.
/// This type pins the digest algorithm and wire form used to prevent
/// equal-generation content divergence.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NetworkPlanDigest([u8; 32]);

impl NetworkPlanDigest {
    /// Digest a canonical provider-neutral plan encoding with SHA-256.
    pub fn sha256(canonical_plan: impl AsRef<[u8]>) -> Self {
        Self(Sha256::digest(canonical_plan.as_ref()).into())
    }

    /// Construct from an already verified SHA-256 value.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Return the raw SHA-256 value.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Display for NetworkPlanDigest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for NetworkPlanDigest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("NetworkPlanDigest")
            .field(&self.to_string())
            .finish()
    }
}

impl FromStr for NetworkPlanDigest {
    type Err = NetworkPlanDigestParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
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
        Ok(Self(bytes))
    }
}

impl Serialize for NetworkPlanDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for NetworkPlanDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Stable reason a serialized plan digest was rejected.
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
/// Resource and capability content is compiled above this low-dependency
/// contract. Its canonical encoding is represented here by `digest`, so the
/// same generation cannot be accepted with different desired content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkPlan {
    plan_id: NetworkPlanId,
    generation: NetworkResourceGeneration,
    digest: NetworkPlanDigest,
}

impl NetworkPlan {
    /// Construct a desired plan envelope from its stable identity, monotonic
    /// generation, and canonical content digest.
    pub fn new(
        plan_id: NetworkPlanId,
        generation: NetworkResourceGeneration,
        digest: NetworkPlanDigest,
    ) -> Self {
        Self {
            plan_id,
            generation,
            digest,
        }
    }

    /// Stable identity of this connectivity plan across generations.
    pub fn plan_id(&self) -> &NetworkPlanId {
        &self.plan_id
    }

    /// Monotonic desired generation.
    pub fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }

    /// Canonical digest of all desired content in this generation.
    pub fn digest(&self) -> NetworkPlanDigest {
        self.digest
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
            std::cmp::Ordering::Equal if candidate.digest != self.digest => {
                Err(NetworkPlanUpdateError::EqualGenerationDigestConflict {
                    generation: self.generation,
                })
            }
            std::cmp::Ordering::Equal => Ok(NetworkPlanUpdate::Idempotent),
            std::cmp::Ordering::Greater => Ok(NetworkPlanUpdate::Advance),
        }
    }
}

/// Accepted relationship between a current and candidate desired plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPlanUpdate {
    /// Same identity, generation, and digest.
    Idempotent,
    /// Same identity with a strictly newer generation.
    Advance,
}

/// Rejection from desired-plan generation/digest validation.
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
    EqualGenerationDigestConflict {
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
            Self::EqualGenerationDigestConflict { generation } => write!(
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

    fn plan_id(value: &str) -> NetworkPlanId {
        value.parse().expect("fixture plan id should parse")
    }

    fn plan(generation: u64, content: &[u8]) -> NetworkPlan {
        NetworkPlan::new(
            plan_id("netplan_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            NetworkResourceGeneration::new(generation),
            NetworkPlanDigest::sha256(content),
        )
    }

    #[test]
    fn sha256_and_wire_form_are_pinned() {
        let digest = NetworkPlanDigest::sha256(b"nimbus-network-plan-v1");

        assert_eq!(
            digest.to_string(),
            "818c3d26850ef542623be2fdabbc9ddc4989d58117d08d0506f67b2c6e4757c5"
        );
        assert_eq!(
            serde_json::to_string(&digest).expect("digest should serialize"),
            r#""818c3d26850ef542623be2fdabbc9ddc4989d58117d08d0506f67b2c6e4757c5""#
        );
        assert_eq!(
            serde_json::from_str::<NetworkPlanDigest>(
                r#""818c3d26850ef542623be2fdabbc9ddc4989d58117d08d0506f67b2c6e4757c5""#
            )
            .expect("digest should deserialize"),
            digest
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
            Err(NetworkPlanUpdateError::EqualGenerationDigestConflict {
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
            NetworkPlanDigest::sha256(b"desired"),
        );

        assert_eq!(
            current.classify_update(&candidate),
            Err(NetworkPlanUpdateError::PlanIdentityMismatch)
        );
    }
}
