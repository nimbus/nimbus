//! Complete network intent and derived phase-reference vocabulary.

use nimbus_network::{NetworkPlanDigest, NetworkPlanId, NetworkResourceGeneration};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{WorkloadSagaIntent, parse_decimal};
use crate::CompiledWorkloadNetworkPlan;

/// Complete provider-neutral network intent carried by one saga generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadNetworkIntent(CompiledWorkloadNetworkPlan);

impl WorkloadNetworkIntent {
    /// Retain one complete validated compiled plan.
    pub fn new(compiled_plan: CompiledWorkloadNetworkPlan) -> Self {
        Self(compiled_plan)
    }

    /// Return the complete validated compiled plan.
    pub fn compiled_plan(&self) -> &CompiledWorkloadNetworkPlan {
        &self.0
    }

    /// Consume the carrier and return the complete validated compiled plan.
    pub fn into_compiled_plan(self) -> CompiledWorkloadNetworkPlan {
        self.0
    }

    /// Derive the stable plan identity from the compiled envelope.
    pub fn plan_id(&self) -> &NetworkPlanId {
        self.0.plan().plan_id()
    }

    /// Derive the desired network generation from the compiled envelope.
    pub fn generation(&self) -> NetworkResourceGeneration {
        self.0.plan().generation()
    }

    /// Derive the complete desired plan digest from the compiled envelope.
    pub fn digest(&self) -> NetworkPlanDigest {
        self.0.plan().digest()
    }
}

impl Serialize for WorkloadNetworkIntent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut wire = serde_json::to_value(&self.0).map_err(serde::ser::Error::custom)?;
        for pointer in ["/plan/generation", "/content/identity/generation"] {
            let generation = wire.pointer_mut(pointer).ok_or_else(|| {
                serde::ser::Error::custom("compiled network plan omitted its generation")
            })?;
            *generation = serde_json::Value::String(self.generation().as_u64().to_string());
        }
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WorkloadNetworkIntent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        crate::network_plan::deserialize_saga_compiled_network_plan(deserializer).map(Self)
    }
}

/// Stable derived reference to network-manager desired state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadNetworkReference {
    plan_id: NetworkPlanId,
    #[serde(with = "network_generation_decimal")]
    generation: NetworkResourceGeneration,
    digest: NetworkPlanDigest,
}

impl WorkloadNetworkReference {
    /// Derive a phase-safe tuple from the complete saga intent.
    pub fn for_intent(intent: &WorkloadSagaIntent) -> Self {
        Self {
            plan_id: intent.network().plan_id().clone(),
            generation: intent.network().generation(),
            digest: intent.network().digest(),
        }
    }

    /// Stable compiled-plan identity.
    pub fn plan_id(&self) -> &NetworkPlanId {
        &self.plan_id
    }

    /// Desired network generation.
    pub fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }

    /// Complete desired plan digest.
    pub fn digest(&self) -> NetworkPlanDigest {
        self.digest
    }
}

mod network_generation_decimal {
    use super::*;

    pub fn serialize<S>(value: &NetworkResourceGeneration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.as_u64().to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<NetworkResourceGeneration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        parse_decimal(
            &value,
            "network generation must be canonical unsigned decimal text",
        )
        .map(NetworkResourceGeneration::new)
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[path = "network/tests.rs"]
mod tests;
