use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{Error, Result};

mod access;
#[cfg(test)]
mod tests;

pub use self::access::{
    AccessAction, AccessOperator, AccessPredicate, AccessRule, AccessValue, CompiledReadRule,
    TableAccessPolicy,
};

/// Normalized authenticated principal context passed from the transport boundary
/// into the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PrincipalContext {
    #[serde(default)]
    pub authenticated: bool,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub claims: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub verified_claims: Map<String, Value>,
}

impl PrincipalContext {
    /// Returns an anonymous principal context.
    pub fn anonymous() -> Self {
        Self::default()
    }

    /// Returns the trusted service principal used for engine-owned work.
    pub fn system() -> Self {
        let mut claims = Map::new();
        claims.insert("sub".to_string(), Value::String("system".to_string()));
        Self {
            authenticated: true,
            claims,
            verified_claims: Map::new(),
        }
    }

    /// Returns a stable snapshot fingerprint for subscription ownership and
    /// conservative invalidation.
    pub fn snapshot(&self) -> Result<PrincipalSnapshot> {
        let bytes =
            serde_json::to_vec(self).map_err(|error| Error::Serialization(error.to_string()))?;
        let digest = Sha256::digest(bytes);
        Ok(PrincipalSnapshot {
            digest: format!("{digest:x}"),
        })
    }

    pub(super) fn claim(&self, source: PrincipalClaimSource, claim: &str) -> Option<&Value> {
        match source {
            PrincipalClaimSource::Identity => self.claims.get(claim),
            PrincipalClaimSource::VerifiedIdentity => self.verified_claims.get(claim),
        }
    }
}

/// Stable fingerprint of the principal context captured when a subscription was
/// registered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalSnapshot {
    pub digest: String,
}

/// Claim bag source inside a normalized principal context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalClaimSource {
    Identity,
    VerifiedIdentity,
}

/// Returns a stable revision fingerprint for a table access policy.
pub fn policy_revision_id(policy: Option<&TableAccessPolicy>) -> Result<String> {
    let bytes =
        serde_json::to_vec(&policy).map_err(|error| Error::Serialization(error.to_string()))?;
    let digest = Sha256::digest(bytes);
    Ok(format!("{digest:x}"))
}
