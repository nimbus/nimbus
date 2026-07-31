//! Authenticated observation returned by the machine port-forwarding adapter.
//!
//! This is provider evidence, not desired configuration. The adapter creates a
//! receipt only after the gvproxy response authenticates the exact provider
//! incarnation, generation, publication, and outcome.

use nimbus_core::TenantId;
use nimbus_network::{NetworkProviderHandle, NetworkResourceGeneration};
use serde::{Deserialize, Serialize};

use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

/// Exact outcome authenticated from one gvproxy forwarding operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachinePortForwardOutcome {
    Exposed,
    Withdrawn,
    ExactAlreadyAbsent,
}

/// Tenant- and sandbox-bound observation of one machine forwarding effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachinePortForwardReceipt {
    pub outcome: MachinePortForwardOutcome,
    pub tenant_id: TenantId,
    pub sandbox_id: SandboxId,
    pub binding: SandboxPortBinding,
    pub provider_instance: NetworkProviderHandle,
    pub provider_generation: NetworkResourceGeneration,
}

impl MachinePortForwardReceipt {
    pub(super) fn authenticated(
        outcome: MachinePortForwardOutcome,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        binding: &SandboxPortBinding,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
    ) -> Self {
        Self {
            outcome,
            tenant_id: tenant_id.clone(),
            sandbox_id: sandbox_id.clone(),
            binding: binding.clone(),
            provider_instance: provider_instance.clone(),
            provider_generation,
        }
    }
}

/// Fresh, read-only observation of the complete desired forwarding batch.
///
/// This type is deliberately not serializable. Durable operation receipts
/// cannot be substituted for a provider observation captured by the current
/// inspection attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurrentMachinePortForwardingObservation {
    provider_instance: NetworkProviderHandle,
    provider_generation: NetworkResourceGeneration,
    receipts: Vec<MachinePortForwardReceipt>,
}

impl CurrentMachinePortForwardingObservation {
    pub(super) fn authenticated(
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
        receipts: Vec<MachinePortForwardReceipt>,
    ) -> Self {
        Self {
            provider_instance: provider_instance.clone(),
            provider_generation,
            receipts,
        }
    }

    pub(crate) fn provider_instance(&self) -> &NetworkProviderHandle {
        &self.provider_instance
    }

    pub(crate) fn provider_generation(&self) -> NetworkResourceGeneration {
        self.provider_generation
    }

    pub(crate) fn receipts(&self) -> &[MachinePortForwardReceipt] {
        &self.receipts
    }
}
