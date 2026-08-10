//! Workload-neutral contracts for exact sandbox execution teardown.
//!
//! The upper workload coordinator owns lifecycle order and policy. This module
//! carries only stable sandbox identity, opaque command fences, and closed
//! provider observations. It performs no provider or network effect.

use nimbus_core::TenantId;
use thiserror::Error;

use crate::{ProviderCommandClaim, ProviderCommandOperation, SandboxExecutionAttemptId, SandboxId};

mod attachment;

pub use attachment::{
    SandboxNetworkReleaseAbsenceEvidence, SandboxNetworkTeardownCommand,
    SandboxNetworkTeardownCommandError, SandboxNetworkTeardownCommandInput,
    SandboxNetworkTeardownIdentity, SandboxNetworkTeardownIdentityInput,
    SandboxNetworkTeardownObservation, SandboxNetworkTeardownOperation,
};

/// One exact execution-only teardown operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxExecutionTeardownOperation {
    Drain,
    Stop,
}

impl SandboxExecutionTeardownOperation {
    pub const fn provider_operation(self) -> ProviderCommandOperation {
        match self {
            Self::Drain => ProviderCommandOperation::DrainExecution,
            Self::Stop => ProviderCommandOperation::StopExecution,
        }
    }
}

/// Exact sandbox-owned input derived from one confirmed upper command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxExecutionTeardownCommand {
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    execution_attempt_id: SandboxExecutionAttemptId,
    provider_registration_key: String,
    operation: SandboxExecutionTeardownOperation,
    provider_claim: ProviderCommandClaim,
}

impl SandboxExecutionTeardownCommand {
    pub fn new(
        tenant_id: TenantId,
        sandbox_id: SandboxId,
        execution_attempt_id: SandboxExecutionAttemptId,
        provider_registration_key: impl Into<String>,
        operation: SandboxExecutionTeardownOperation,
        provider_claim: ProviderCommandClaim,
    ) -> Result<Self, SandboxExecutionTeardownCommandError> {
        let provider_registration_key = provider_registration_key.into();
        if provider_registration_key.trim().is_empty() {
            return Err(SandboxExecutionTeardownCommandError::EmptyProviderRegistrationKey);
        }
        if provider_claim.operation() != operation.provider_operation() {
            return Err(SandboxExecutionTeardownCommandError::OperationMismatch {
                command: operation,
                claim: provider_claim.operation(),
            });
        }
        Ok(Self {
            tenant_id,
            sandbox_id,
            execution_attempt_id,
            provider_registration_key,
            operation,
            provider_claim,
        })
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn sandbox_id(&self) -> &SandboxId {
        &self.sandbox_id
    }

    pub fn execution_attempt_id(&self) -> &SandboxExecutionAttemptId {
        &self.execution_attempt_id
    }

    pub fn provider_registration_key(&self) -> &str {
        &self.provider_registration_key
    }

    pub const fn operation(&self) -> SandboxExecutionTeardownOperation {
        self.operation
    }

    pub fn provider_claim(&self) -> &ProviderCommandClaim {
        &self.provider_claim
    }
}

/// Closed provider observation before compute translates it to saga evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxExecutionTeardownObservation {
    Succeeded { evidence: Vec<u8> },
    DefiniteFailure { code: String, evidence: Vec<u8> },
    Absent { evidence: Vec<u8> },
    RetryAuthorized { evidence: Vec<u8> },
    InProgress { evidence: Vec<u8> },
    Ambiguous { evidence: Vec<u8> },
}

impl SandboxExecutionTeardownObservation {
    pub fn evidence(&self) -> &[u8] {
        match self {
            Self::Succeeded { evidence }
            | Self::DefiniteFailure { evidence, .. }
            | Self::Absent { evidence }
            | Self::RetryAuthorized { evidence }
            | Self::InProgress { evidence }
            | Self::Ambiguous { evidence } => evidence,
        }
    }

    /// Stable failure code when the provider rejected the exact command.
    pub fn failure_code(&self) -> Option<&str> {
        match self {
            Self::DefiniteFailure { code, .. } => Some(code),
            Self::Succeeded { .. }
            | Self::Absent { .. }
            | Self::RetryAuthorized { .. }
            | Self::InProgress { .. }
            | Self::Ambiguous { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SandboxExecutionTeardownCommandError {
    #[error("sandbox teardown provider registration key must not be empty")]
    EmptyProviderRegistrationKey,
    #[error("sandbox teardown command operation {command:?} conflicts with claim {claim:?}")]
    OperationMismatch {
        command: SandboxExecutionTeardownOperation,
        claim: ProviderCommandOperation,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProviderCommandClaimInput, ProviderCommandOperation};

    fn claim(operation: ProviderCommandOperation) -> ProviderCommandClaim {
        ProviderCommandClaim::new(ProviderCommandClaimInput {
            authority_id: "01jtestauthority0000000000000".to_owned(),
            effect_subject: "{\"execution\":\"fixture\"}".to_owned(),
            source_attempt_id: None,
            attempt_id: "01jtestattempt00000000000000".to_owned(),
            dispatch_epoch: 1,
            workload_generation: 3,
            restart_ordinal: 0,
            desired_digest: "1".repeat(64),
            source_digest: "2".repeat(64),
            network_plan_digest: "3".repeat(64),
            provider_target_digest: "4".repeat(64),
            operation,
        })
        .expect("fixture provider claim should validate")
    }

    #[test]
    fn neutral_teardown_command_requires_exact_operation() {
        let command = SandboxExecutionTeardownCommand::new(
            TenantId::new("tenant-a").expect("tenant should validate"),
            SandboxId::new("sandbox-a"),
            SandboxExecutionAttemptId::new("attempt-a").expect("attempt should validate"),
            "nimbus-sandbox.container-execution",
            SandboxExecutionTeardownOperation::Drain,
            claim(ProviderCommandOperation::DrainExecution),
        )
        .expect("matching command should validate");
        assert_eq!(
            command.operation(),
            SandboxExecutionTeardownOperation::Drain
        );

        assert!(matches!(
            SandboxExecutionTeardownCommand::new(
                TenantId::new("tenant-a").expect("tenant should validate"),
                SandboxId::new("sandbox-a"),
                SandboxExecutionAttemptId::new("attempt-a").expect("attempt should validate"),
                "nimbus-sandbox.container-execution",
                SandboxExecutionTeardownOperation::Drain,
                claim(ProviderCommandOperation::StopExecution),
            ),
            Err(SandboxExecutionTeardownCommandError::OperationMismatch { .. })
        ));
    }
}
