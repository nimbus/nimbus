//! Narrow parent-side retirement capability for one exact gvproxy batch.

use super::*;

/// Fresh complete-batch forwarding state under one provider generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachinePortForwardingRetirementObservation {
    Present(Vec<MachinePortForwardReceipt>),
    Partial {
        present: Vec<MachinePortForwardReceipt>,
        absent: Vec<MachinePortForwardReceipt>,
    },
    Absent(Vec<MachinePortForwardReceipt>),
}

impl MachinePortForwardingRetirementObservation {
    pub fn absent_receipts(&self) -> Option<&[MachinePortForwardReceipt]> {
        match self {
            Self::Absent(receipts) => Some(receipts),
            Self::Present(_) | Self::Partial { .. } => None,
        }
    }
}

/// Provider-effect port for parent-local final forwarding withdrawal.
pub trait MachinePortForwardingRetirement: Send + Sync {
    fn provider_instance(&self) -> &NetworkProviderHandle;
    fn provider_generation(&self) -> NetworkResourceGeneration;
    fn inspect_batch(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
    ) -> Result<MachinePortForwardingRetirementObservation>;
    fn withdraw_batch(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
    ) -> Result<Vec<MachinePortForwardReceipt>>;
}

/// Real gvproxy-backed final forwarding retirement capability.
#[derive(Debug, Clone)]
pub struct OciMachinePortForwardingRetirement {
    config: OciMachinePortForwarderConfig,
}

impl OciMachinePortForwardingRetirement {
    pub fn new(config: OciMachinePortForwarderConfig) -> Self {
        Self { config }
    }
}

impl MachinePortForwardingRetirement for OciMachinePortForwardingRetirement {
    fn provider_instance(&self) -> &NetworkProviderHandle {
        self.config.provider_instance()
    }

    fn provider_generation(&self) -> NetworkResourceGeneration {
        self.config.provider_generation()
    }

    fn inspect_batch(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
    ) -> Result<MachinePortForwardingRetirementObservation> {
        let observation = self.config.inspect(tenant_id, sandbox_id, bindings)?;
        authenticate_observation_identity(&self.config, &observation)?;
        let mut present = Vec::new();
        let mut absent = Vec::new();
        for slot in observation.slots() {
            if let Some(receipt) = slot.exposed_receipt() {
                present.push(receipt.clone());
            } else if let Some(receipt) = slot.absent_receipt() {
                absent.push(receipt.clone());
            } else {
                return Err(current_observation_error(
                    self.provider_generation(),
                    slot.conflict_detail()
                        .unwrap_or("provider returned a conflicting forwarding slot"),
                ));
            }
        }
        if present.is_empty() {
            Ok(MachinePortForwardingRetirementObservation::Absent(absent))
        } else if absent.is_empty() {
            Ok(MachinePortForwardingRetirementObservation::Present(present))
        } else {
            Ok(MachinePortForwardingRetirementObservation::Partial { present, absent })
        }
    }

    fn withdraw_batch(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
    ) -> Result<Vec<MachinePortForwardReceipt>> {
        converge_machine_ports_without_journal(
            &self.config,
            tenant_id,
            sandbox_id,
            bindings,
            MachinePortForwardingAction::Withdraw,
        )
    }
}
