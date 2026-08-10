//! Workload-neutral contracts for exact network attachment teardown.

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAttachmentId, NetworkCapabilitySourceDigest, NetworkPlan, NetworkProviderId,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{ProviderCommandClaim, ProviderCommandOperation, SandboxExecutionAttemptId, SandboxId};

const NETWORK_TEARDOWN_TARGET_DOMAIN: &[u8] =
    b"nimbus.sandbox.network-teardown.provider-target.v1\0";

/// One exact attachment-only teardown operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxNetworkTeardownOperation {
    Detach,
    Release,
}

impl SandboxNetworkTeardownOperation {
    pub const fn provider_operation(self) -> ProviderCommandOperation {
        match self {
            Self::Detach => ProviderCommandOperation::DetachNetwork,
            Self::Release => ProviderCommandOperation::ReleaseNetwork,
        }
    }
}

/// Complete typed input for one exact attachment identity.
pub struct SandboxNetworkTeardownIdentityInput {
    pub tenant_id: TenantId,
    pub sandbox_id: SandboxId,
    pub execution_attempt_id: SandboxExecutionAttemptId,
    pub attachment_id: NetworkAttachmentId,
    pub network_plan: NetworkPlan,
    pub provider_registration_key: String,
    pub provider_source_digest: NetworkCapabilitySourceDigest,
}

/// Stable typed fences shared by detach and release for one attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxNetworkTeardownIdentity {
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    execution_attempt_id: SandboxExecutionAttemptId,
    attachment_id: NetworkAttachmentId,
    network_plan: NetworkPlan,
    provider_registration_key: String,
    provider_source_digest: NetworkCapabilitySourceDigest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkTeardownEffectSubject<'a> {
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    execution_attempt_id: &'a SandboxExecutionAttemptId,
    attachment_id: &'a NetworkAttachmentId,
    network_plan_id: &'a nimbus_network::NetworkPlanId,
    network_generation: nimbus_network::NetworkResourceGeneration,
    network_plan_digest: nimbus_network::NetworkPlanDigest,
    provider_registration_key: &'a str,
    provider_source_digest: NetworkCapabilitySourceDigest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkTeardownProviderTarget<'a> {
    provider_registration_key: &'a str,
    provider_source_digest: NetworkCapabilitySourceDigest,
}

impl SandboxNetworkTeardownIdentity {
    pub fn new(
        input: SandboxNetworkTeardownIdentityInput,
    ) -> Result<Self, SandboxNetworkTeardownCommandError> {
        if input.provider_registration_key.trim().is_empty() {
            return Err(SandboxNetworkTeardownCommandError::EmptyProviderRegistrationKey);
        }
        Ok(Self {
            tenant_id: input.tenant_id,
            sandbox_id: input.sandbox_id,
            execution_attempt_id: input.execution_attempt_id,
            attachment_id: input.attachment_id,
            network_plan: input.network_plan,
            provider_registration_key: input.provider_registration_key,
            provider_source_digest: input.provider_source_digest,
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

    pub fn attachment_id(&self) -> &NetworkAttachmentId {
        &self.attachment_id
    }

    pub fn network_plan(&self) -> &NetworkPlan {
        &self.network_plan
    }

    pub fn provider_registration_key(&self) -> &str {
        &self.provider_registration_key
    }

    pub fn provider_id(&self) -> NetworkProviderId {
        NetworkProviderId::for_registration_key(&self.provider_registration_key)
    }

    pub const fn provider_source_digest(&self) -> NetworkCapabilitySourceDigest {
        self.provider_source_digest
    }

    /// Canonical identity shared by the two independent command streams.
    pub fn provider_effect_subject(&self) -> String {
        serde_json::to_string(&NetworkTeardownEffectSubject {
            tenant_id: &self.tenant_id,
            sandbox_id: &self.sandbox_id,
            execution_attempt_id: &self.execution_attempt_id,
            attachment_id: &self.attachment_id,
            network_plan_id: self.network_plan.plan_id(),
            network_generation: self.network_plan.generation(),
            network_plan_digest: self.network_plan.digest(),
            provider_registration_key: &self.provider_registration_key,
            provider_source_digest: self.provider_source_digest,
        })
        .expect("closed network teardown identity always serializes")
    }

    /// Digest that binds the provider claim to the selected attachment owner.
    pub fn provider_target_digest(&self) -> String {
        let target = serde_json::to_vec(&NetworkTeardownProviderTarget {
            provider_registration_key: &self.provider_registration_key,
            provider_source_digest: self.provider_source_digest,
        })
        .expect("closed network teardown target always serializes");
        let mut digest = Sha256::new();
        digest.update(NETWORK_TEARDOWN_TARGET_DOMAIN);
        digest.update(target);
        format!("{:x}", digest.finalize())
    }
}

/// Complete typed input for one exact network teardown command.
pub struct SandboxNetworkTeardownCommandInput {
    pub identity: SandboxNetworkTeardownIdentity,
    pub operation: SandboxNetworkTeardownOperation,
    pub provider_claim: ProviderCommandClaim,
}

/// Exact sandbox-owned input derived from one confirmed upper command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxNetworkTeardownCommand {
    identity: SandboxNetworkTeardownIdentity,
    operation: SandboxNetworkTeardownOperation,
    provider_claim: ProviderCommandClaim,
}

impl SandboxNetworkTeardownCommand {
    pub fn new(
        input: SandboxNetworkTeardownCommandInput,
    ) -> Result<Self, SandboxNetworkTeardownCommandError> {
        if input.provider_claim.operation() != input.operation.provider_operation() {
            return Err(SandboxNetworkTeardownCommandError::OperationMismatch {
                command: input.operation,
                claim: input.provider_claim.operation(),
            });
        }
        if input.provider_claim.network_plan_digest()
            != input.identity.network_plan().digest().to_string()
        {
            return Err(SandboxNetworkTeardownCommandError::NetworkPlanDigestMismatch);
        }
        if input.provider_claim.workload_generation()
            != input.identity.network_plan().generation().as_u64()
        {
            return Err(SandboxNetworkTeardownCommandError::NetworkGenerationMismatch);
        }
        if input.provider_claim.effect_subject() != input.identity.provider_effect_subject() {
            return Err(SandboxNetworkTeardownCommandError::EffectSubjectMismatch);
        }
        if input.provider_claim.provider_target_digest() != input.identity.provider_target_digest()
        {
            return Err(SandboxNetworkTeardownCommandError::ProviderTargetMismatch);
        }
        Ok(Self {
            identity: input.identity,
            operation: input.operation,
            provider_claim: input.provider_claim,
        })
    }

    pub fn tenant_id(&self) -> &TenantId {
        self.identity.tenant_id()
    }

    pub fn sandbox_id(&self) -> &SandboxId {
        self.identity.sandbox_id()
    }

    pub fn execution_attempt_id(&self) -> &SandboxExecutionAttemptId {
        self.identity.execution_attempt_id()
    }

    pub fn attachment_id(&self) -> &NetworkAttachmentId {
        self.identity.attachment_id()
    }

    pub fn network_plan(&self) -> &NetworkPlan {
        self.identity.network_plan()
    }

    pub fn provider_registration_key(&self) -> &str {
        self.identity.provider_registration_key()
    }

    pub fn provider_id(&self) -> NetworkProviderId {
        self.identity.provider_id()
    }

    pub const fn provider_source_digest(&self) -> NetworkCapabilitySourceDigest {
        self.identity.provider_source_digest()
    }

    pub const fn operation(&self) -> SandboxNetworkTeardownOperation {
        self.operation
    }

    pub fn provider_claim(&self) -> &ProviderCommandClaim {
        &self.provider_claim
    }
}

/// Closed provider observation before compute translates it to saga evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxNetworkTeardownObservation {
    Succeeded { evidence: Vec<u8> },
    DefiniteFailure { code: String, evidence: Vec<u8> },
    Absent { evidence: Vec<u8> },
    RetryAuthorized { evidence: Vec<u8> },
    InProgress { evidence: Vec<u8> },
    Ambiguous { evidence: Vec<u8> },
}

impl SandboxNetworkTeardownObservation {
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
pub enum SandboxNetworkTeardownCommandError {
    #[error("sandbox network teardown provider registration key must not be empty")]
    EmptyProviderRegistrationKey,
    #[error(
        "sandbox network teardown command operation {command:?} conflicts with claim {claim:?}"
    )]
    OperationMismatch {
        command: SandboxNetworkTeardownOperation,
        claim: ProviderCommandOperation,
    },
    #[error("sandbox network teardown command has a crossed network plan digest")]
    NetworkPlanDigestMismatch,
    #[error("sandbox network teardown command has a crossed network generation")]
    NetworkGenerationMismatch,
    #[error("sandbox network teardown command has a crossed effect subject")]
    EffectSubjectMismatch,
    #[error("sandbox network teardown command has a crossed provider target")]
    ProviderTargetMismatch,
}

#[cfg(test)]
mod tests {
    use nimbus_network::{
        NetworkAttachmentId, NetworkCapabilitySourceDigest, NetworkPlanContentDigest,
        NetworkPlanId, NetworkResourceGeneration,
    };

    use super::*;
    use crate::backends::{
        CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY, sandbox_network_plan_requirements,
    };
    use crate::{ProviderCommandClaimInput, SandboxBackendKind};

    fn plan_with_content(generation: u64, content: &[u8]) -> NetworkPlan {
        let tenant_id = TenantId::new("tenant-a").expect("tenant should validate");
        NetworkPlan::new(
            NetworkPlanId::for_tenant_workload_plan(&tenant_id, "workload-a"),
            NetworkResourceGeneration::new(generation),
            NetworkPlanContentDigest::sha256(content),
            sandbox_network_plan_requirements(SandboxBackendKind::Container)
                .capability_requirements()
                .clone(),
        )
    }

    fn plan(generation: u64) -> NetworkPlan {
        plan_with_content(generation, b"network-plan-a")
    }

    fn claim(
        identity: &SandboxNetworkTeardownIdentity,
        operation: ProviderCommandOperation,
    ) -> ProviderCommandClaim {
        ProviderCommandClaim::new(ProviderCommandClaimInput {
            authority_id: "01jnetworkauthority00000000000".to_owned(),
            effect_subject: identity.provider_effect_subject(),
            source_attempt_id: None,
            attempt_id: "01jnetworkattempt000000000000".to_owned(),
            dispatch_epoch: 1,
            workload_generation: identity.network_plan().generation().as_u64(),
            restart_ordinal: 0,
            desired_digest: "1".repeat(64),
            source_digest: "2".repeat(64),
            network_plan_digest: identity.network_plan().digest().to_string(),
            provider_target_digest: identity.provider_target_digest(),
            operation,
        })
        .expect("fixture provider claim should validate")
    }

    fn input(
        network_plan: NetworkPlan,
        operation: SandboxNetworkTeardownOperation,
    ) -> SandboxNetworkTeardownCommandInput {
        let provider_operation = operation.provider_operation();
        let identity = SandboxNetworkTeardownIdentity::new(SandboxNetworkTeardownIdentityInput {
            tenant_id: TenantId::new("tenant-a").expect("tenant should validate"),
            sandbox_id: SandboxId::new("sandbox-a"),
            execution_attempt_id: SandboxExecutionAttemptId::new("attempt-a")
                .expect("attempt should validate"),
            attachment_id: NetworkAttachmentId::for_workload_attachment("workload-a", "default"),
            provider_registration_key: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY.to_owned(),
            provider_source_digest: NetworkCapabilitySourceDigest::from_bytes([3; 32]),
            network_plan,
        })
        .expect("fixture identity should validate");
        SandboxNetworkTeardownCommandInput {
            provider_claim: claim(&identity, provider_operation),
            identity,
            operation,
        }
    }

    fn claim_with(
        source: &ProviderCommandClaim,
        effect_subject: Option<String>,
        provider_target_digest: Option<String>,
    ) -> ProviderCommandClaim {
        ProviderCommandClaim::new(ProviderCommandClaimInput {
            authority_id: source.authority_id().to_owned(),
            effect_subject: effect_subject.unwrap_or_else(|| source.effect_subject().to_owned()),
            source_attempt_id: source.source_attempt_id().map(str::to_owned),
            attempt_id: source.attempt_id().to_owned(),
            dispatch_epoch: source.dispatch_epoch(),
            workload_generation: source.workload_generation(),
            restart_ordinal: source.restart_ordinal(),
            desired_digest: source.desired_digest().to_owned(),
            source_digest: source.source_digest().to_owned(),
            network_plan_digest: source.network_plan_digest().to_owned(),
            provider_target_digest: provider_target_digest
                .unwrap_or_else(|| source.provider_target_digest().to_owned()),
            operation: source.operation(),
        })
        .expect("crossed fixture claim should remain intrinsically valid")
    }

    #[test]
    fn exact_network_teardown_contract_rejects_crossed_operation_plan_and_generation() {
        let network_plan = plan(3);
        let command = SandboxNetworkTeardownCommand::new(input(
            network_plan.clone(),
            SandboxNetworkTeardownOperation::Detach,
        ))
        .expect("exact command should validate");
        assert_eq!(
            command.provider_id(),
            NetworkProviderId::for_registration_key(CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY)
        );
        assert_eq!(command.network_plan(), &network_plan);

        let mut crossed_operation = input(
            network_plan.clone(),
            SandboxNetworkTeardownOperation::Release,
        );
        crossed_operation.provider_claim = claim(
            &crossed_operation.identity,
            ProviderCommandOperation::DetachNetwork,
        );
        assert!(matches!(
            SandboxNetworkTeardownCommand::new(crossed_operation),
            Err(SandboxNetworkTeardownCommandError::OperationMismatch { .. })
        ));

        let mut crossed_plan = input(
            network_plan.clone(),
            SandboxNetworkTeardownOperation::Detach,
        );
        crossed_plan.identity.network_plan = plan_with_content(3, b"crossed-network-plan");
        assert_eq!(
            SandboxNetworkTeardownCommand::new(crossed_plan),
            Err(SandboxNetworkTeardownCommandError::NetworkPlanDigestMismatch)
        );

        let mut crossed_generation = input(
            network_plan.clone(),
            SandboxNetworkTeardownOperation::Detach,
        );
        crossed_generation.identity.network_plan = plan(4);
        assert_eq!(
            SandboxNetworkTeardownCommand::new(crossed_generation),
            Err(SandboxNetworkTeardownCommandError::NetworkGenerationMismatch)
        );

        let mut crossed_subject = input(
            network_plan.clone(),
            SandboxNetworkTeardownOperation::Detach,
        );
        crossed_subject.provider_claim = claim_with(
            &crossed_subject.provider_claim,
            Some("{\"attachment\":\"crossed\"}".to_owned()),
            None,
        );
        assert_eq!(
            SandboxNetworkTeardownCommand::new(crossed_subject),
            Err(SandboxNetworkTeardownCommandError::EffectSubjectMismatch)
        );

        let mut crossed_target = input(network_plan, SandboxNetworkTeardownOperation::Detach);
        crossed_target.provider_claim =
            claim_with(&crossed_target.provider_claim, None, Some("9".repeat(64)));
        assert_eq!(
            SandboxNetworkTeardownCommand::new(crossed_target),
            Err(SandboxNetworkTeardownCommandError::ProviderTargetMismatch)
        );
    }
}
