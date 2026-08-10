//! Durable parent-publication retirement progression and immutable evidence.

use nimbus::{Error, SandboxPortBinding};
use nimbus_compute::workload_saga::ConfirmedWorkloadTeardownCommand;
use nimbus_compute::workload_saga::teardown_provider_command::ConfirmedTeardownProviderCommand;
use nimbus_machine::MachineForwarderAuthority;
use nimbus_machine::api::{
    MachineApiNetworkReleaseAbsenceEvidence, MachineApiWorkloadTeardownExecuteObservation,
    MachineApiWorkloadTeardownInspectObservation, MachineApiWorkloadTeardownObservation,
    MachineApiWorkloadTeardownPhaseRequest, MachineApiWorkloadTeardownPhaseResponse,
    MachineApiWorkloadTeardownRequestDigest,
};
use nimbus_network::{PortLeasePhase, PortLeaseRecord};
use nimbus_sandbox::{
    MachinePortForwardOutcome, MachinePortForwardReceipt, ProviderCommandClaim,
    ProviderCommandOperation,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, WorkloadExecutionReference, WorkloadOwnerEvidenceDigest,
    WorkloadProvisionSourceEvidence, WorkloadSagaRevision, WorkloadSagaTransitionId,
    WorkloadTeardownClaim, WorkloadTeardownCommandId, WorkloadTeardownCommandMode,
    WorkloadTeardownReceiptPrefix, WorkloadTeardownStep, WorkloadTeardownSuccessEvidence,
};
use serde::{Deserialize, Serialize};

use super::ConfirmedMachinePublicationMember;

const COMMAND_FENCE_DOMAIN: &[u8] = b"nimbus.machine.retirement.command-fence.v1\0";
const MEMBER_BATCH_DOMAIN: &[u8] = b"nimbus.machine.retirement.member-batch.v1\0";
const FORWARDING_BINDING_BATCH_DOMAIN: &[u8] =
    b"nimbus.machine.retirement.forwarding-binding-batch.v1\0";
const FORWARDING_ABSENCE_DOMAIN: &[u8] = b"nimbus.machine.retirement.forwarding-absence.v1\0";
const PORT_BATCH_DOMAIN: &[u8] = b"nimbus.machine.retirement.port-batch.v1\0";
const RESPONSE_DOMAIN: &[u8] = b"nimbus.machine.retirement.guest-response.v1\0";

/// External phase projection. Durable state uses the data-carrying progress
/// enum below so a phase cannot exist without its required fences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConfirmedMachinePublicationRetirementPhase {
    Active,
    WithdrawalMayExist,
    WithdrawnRetained,
    ReleaseMayExist,
    Released,
}

impl ConfirmedMachinePublicationRetirementPhase {
    pub(crate) const fn is_released(self) -> bool {
        matches!(self, Self::Released)
    }
}

/// Exact durable publication and lease progression for one workload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ConfirmedMachinePublicationRetirementProgress {
    Active,
    WithdrawalMayExist {
        withdrawal: ConfirmedParentWithdrawalFence,
    },
    WithdrawnRetained {
        withdrawal: ConfirmedParentWithdrawalFence,
        withdrawn: ConfirmedParentWithdrawalEvidence,
    },
    ReleaseMayExist {
        withdrawal: ConfirmedParentWithdrawalFence,
        withdrawn: ConfirmedParentWithdrawalEvidence,
        release: ConfirmedGuestReleaseEvidence,
    },
    Released {
        withdrawal: ConfirmedParentWithdrawalFence,
        withdrawn: ConfirmedParentWithdrawalEvidence,
        release: ConfirmedGuestReleaseEvidence,
        parent_ports: ConfirmedParentPortBatchEvidence,
    },
    /// Temporary owner for the coarse stop path. Exact progression can never
    /// enter or overwrite this terminal state.
    LegacyReleased,
}

impl ConfirmedMachinePublicationRetirementProgress {
    pub(super) const fn phase(&self) -> ConfirmedMachinePublicationRetirementPhase {
        match self {
            Self::Active => ConfirmedMachinePublicationRetirementPhase::Active,
            Self::WithdrawalMayExist { .. } => {
                ConfirmedMachinePublicationRetirementPhase::WithdrawalMayExist
            }
            Self::WithdrawnRetained { .. } => {
                ConfirmedMachinePublicationRetirementPhase::WithdrawnRetained
            }
            Self::ReleaseMayExist { .. } => {
                ConfirmedMachinePublicationRetirementPhase::ReleaseMayExist
            }
            Self::Released { .. } | Self::LegacyReleased => {
                ConfirmedMachinePublicationRetirementPhase::Released
            }
        }
    }

    pub(super) fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Active | Self::LegacyReleased => Ok(()),
            Self::WithdrawalMayExist { withdrawal } => withdrawal.validate(),
            Self::WithdrawnRetained {
                withdrawal,
                withdrawn,
            } => {
                withdrawal.validate()?;
                withdrawn.validate_for(withdrawal)
            }
            Self::ReleaseMayExist {
                withdrawal,
                withdrawn,
                release,
            } => {
                withdrawal.validate()?;
                withdrawn.validate_for(withdrawal)?;
                release.validate_for(withdrawal, withdrawn)
            }
            Self::Released {
                withdrawal,
                withdrawn,
                release,
                parent_ports,
            } => {
                withdrawal.validate()?;
                withdrawn.validate_for(withdrawal)?;
                release.validate_for(withdrawal, withdrawn)?;
                parent_ports.validate_released(withdrawal.member_count)
            }
        }
    }

    pub(super) fn withdrawal(&self) -> Option<&ConfirmedParentWithdrawalFence> {
        match self {
            Self::Active | Self::LegacyReleased => None,
            Self::WithdrawalMayExist { withdrawal }
            | Self::WithdrawnRetained { withdrawal, .. }
            | Self::ReleaseMayExist { withdrawal, .. }
            | Self::Released { withdrawal, .. } => Some(withdrawal),
        }
    }

    pub(super) fn authenticate_members(
        &self,
        members: &[ConfirmedMachinePublicationMember],
        forwarding_bindings: &[SandboxPortBinding],
    ) -> Result<(), Error> {
        if let Some(withdrawal) = self.withdrawal()
            && (usize::try_from(withdrawal.member_count).map_err(|_| {
                Error::PreconditionFailed(
                    "confirmed parent withdrawal member count is not representable".to_owned(),
                )
            })? != members.len()
                || withdrawal.members_digest != member_batch_digest(members)?
                || usize::try_from(withdrawal.forwarding_binding_count).map_err(|_| {
                    Error::PreconditionFailed(
                        "confirmed forwarding binding count is not representable".to_owned(),
                    )
                })? != forwarding_bindings.len()
                || withdrawal.forwarding_bindings_digest
                    != forwarding_binding_batch_digest(forwarding_bindings)?)
        {
            return Err(Error::PreconditionFailed(
                "confirmed machine retirement progression crosses publication or forwarding membership"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct ConfirmedTeardownCommandFence {
    command_id: WorkloadTeardownCommandId,
    confirmed_revision: WorkloadSagaRevision,
    confirmed_transition_id: WorkloadSagaTransitionId,
    source: WorkloadProvisionSourceEvidence,
    compiled_network_plan: CompiledWorkloadNetworkPlan,
    execution_locator: WorkloadExecutionReference,
    prior_receipt_prefix: WorkloadTeardownReceiptPrefix,
    mode: WorkloadTeardownCommandMode,
    claim: WorkloadTeardownClaim,
    provider_claim: ProviderCommandClaim,
    command_digest: WorkloadOwnerEvidenceDigest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandFenceDigestPayload<'a> {
    command_id: WorkloadTeardownCommandId,
    confirmed_revision: WorkloadSagaRevision,
    confirmed_transition_id: &'a WorkloadSagaTransitionId,
    source: &'a WorkloadProvisionSourceEvidence,
    compiled_network_plan: &'a CompiledWorkloadNetworkPlan,
    execution_locator: &'a WorkloadExecutionReference,
    prior_receipt_prefix: &'a WorkloadTeardownReceiptPrefix,
    mode: WorkloadTeardownCommandMode,
    claim: &'a WorkloadTeardownClaim,
    provider_claim: &'a ProviderCommandClaim,
}

impl ConfirmedTeardownCommandFence {
    pub(super) fn new(
        command: &ConfirmedWorkloadTeardownCommand,
        provider: &ConfirmedTeardownProviderCommand,
    ) -> Result<Self, Error> {
        if provider.mode() != command.mode() {
            return Err(Error::conflict(
                "retirement command mode crosses its provider claim",
            ));
        }
        let mut fence = Self {
            command_id: command.command_id(),
            confirmed_revision: command.confirmed_revision(),
            confirmed_transition_id: command.confirmed_transition_id().clone(),
            source: command.source().clone(),
            compiled_network_plan: command.compiled_network_plan().clone(),
            execution_locator: command.execution_locator().clone(),
            prior_receipt_prefix: command.prior_receipt_prefix().clone(),
            mode: command.mode(),
            claim: command.claim().clone(),
            provider_claim: provider.claim().clone(),
            command_digest: WorkloadOwnerEvidenceDigest::sha256(b"uninitialized"),
        };
        fence.command_digest = fence.derive_digest()?;
        fence.validate()?;
        Ok(fence)
    }

    fn derive_digest(&self) -> Result<WorkloadOwnerEvidenceDigest, Error> {
        domain_digest(
            COMMAND_FENCE_DOMAIN,
            &CommandFenceDigestPayload {
                command_id: self.command_id,
                confirmed_revision: self.confirmed_revision,
                confirmed_transition_id: &self.confirmed_transition_id,
                source: &self.source,
                compiled_network_plan: &self.compiled_network_plan,
                execution_locator: &self.execution_locator,
                prior_receipt_prefix: &self.prior_receipt_prefix,
                mode: self.mode,
                claim: &self.claim,
                provider_claim: &self.provider_claim,
            },
        )
    }

    fn validate(&self) -> Result<(), Error> {
        let expected_command_id = WorkloadTeardownCommandId::for_confirmed_dispatch(
            &self.claim,
            self.confirmed_revision,
            &self.confirmed_transition_id,
            self.mode,
        )
        .map_err(|error| Error::PreconditionFailed(error.to_string()))?;
        let attempt = self.claim.attempt();
        let plan = self.compiled_network_plan.plan();
        self.prior_receipt_prefix
            .validate_for_claim(&self.claim)
            .map_err(|error| Error::PreconditionFailed(error.to_string()))?;
        let required_steps = required_prior_receipt_steps(attempt.step());
        let effect_subject = serde_json::to_string(&(
            &self.execution_locator,
            attempt.subjects(),
            &self.prior_receipt_prefix,
        ))
        .map_err(|error| {
            Error::Internal(format!(
                "failed to encode confirmed retirement effect subject: {error}"
            ))
        })?;
        let provider_target_digest = WorkloadOwnerEvidenceDigest::sha256(
            serde_json::to_vec(self.claim.provider_target()).map_err(|error| {
                Error::Internal(format!(
                    "failed to encode confirmed retirement provider target: {error}"
                ))
            })?,
        )
        .to_string();
        if self.command_id != expected_command_id
            || self.prior_receipt_prefix.receipts().len() != required_steps.len()
            || required_steps
                .iter()
                .any(|step| self.prior_receipt_prefix.receipt_for(*step).is_none())
            || self.source.source_digest() != attempt.source_digest()
            || self.source.execution_provider_id() != attempt.execution_provider_id()
            || plan.digest() != attempt.network_plan_digest()
            || plan.generation().as_u64() != attempt.generation().as_u64()
            || self.execution_locator.generation() != attempt.generation()
            || self.execution_locator.desired_digest() != attempt.desired_digest()
            || self.provider_claim.authority_id() != attempt.saga_id().as_str()
            || self.provider_claim.attempt_id() != attempt.attempt_id().as_str()
            || self.provider_claim.dispatch_epoch() != self.claim.dispatch_epoch().as_u64()
            || self.provider_claim.workload_generation() != attempt.generation().as_u64()
            || self.provider_claim.desired_digest() != attempt.desired_digest().to_string()
            || self.provider_claim.source_digest() != attempt.source_digest().to_string()
            || self.provider_claim.network_plan_digest()
                != attempt.network_plan_digest().to_string()
            || self.provider_claim.effect_subject() != effect_subject
            || self.provider_claim.provider_target_digest() != provider_target_digest
            || self.provider_claim.operation() != provider_operation(attempt.step())
            || self.command_digest != self.derive_digest()?
        {
            return Err(Error::PreconditionFailed(
                "confirmed machine retirement command fence is corrupt or crossed".to_owned(),
            ));
        }
        Ok(())
    }

    fn same_provider_stream(&self, candidate: &Self) -> bool {
        let left = &self.provider_claim;
        let right = &candidate.provider_claim;
        let epoch_matches = right.dispatch_epoch() == left.dispatch_epoch()
            || left
                .dispatch_epoch()
                .checked_add(1)
                .is_some_and(|next| right.dispatch_epoch() == next);
        left.authority_id() == right.authority_id()
            && left.effect_subject() == right.effect_subject()
            && left.source_attempt_id() == right.source_attempt_id()
            && left.attempt_id() == right.attempt_id()
            && left.workload_generation() == right.workload_generation()
            && left.restart_ordinal() == right.restart_ordinal()
            && left.desired_digest() == right.desired_digest()
            && left.source_digest() == right.source_digest()
            && left.network_plan_digest() == right.network_plan_digest()
            && left.provider_target_digest() == right.provider_target_digest()
            && left.operation() == right.operation()
            && epoch_matches
    }

    fn same_workload_lifecycle(&self, candidate: &Self) -> bool {
        let left = self.claim.attempt();
        let right = candidate.claim.attempt();
        left.key() == right.key()
            && left.saga_id() == right.saga_id()
            && left.generation() == right.generation()
            && left.desired_digest() == right.desired_digest()
            && left.required_node() == right.required_node()
            && left.source_digest() == right.source_digest()
            && left.execution_provider_id() == right.execution_provider_id()
            && left.network_plan_digest() == right.network_plan_digest()
            && left.selection_evidence() == right.selection_evidence()
            && left.cause() == right.cause()
            && self.source == candidate.source
            && self.compiled_network_plan == candidate.compiled_network_plan
            && self.execution_locator == candidate.execution_locator
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct ConfirmedParentWithdrawalFence {
    command: ConfirmedTeardownCommandFence,
    member_count: u32,
    members_digest: WorkloadOwnerEvidenceDigest,
    forwarding_binding_count: u32,
    forwarding_bindings_digest: WorkloadOwnerEvidenceDigest,
}

impl ConfirmedParentWithdrawalFence {
    pub(super) fn new(
        command: &ConfirmedWorkloadTeardownCommand,
        provider: &ConfirmedTeardownProviderCommand,
        members: &[ConfirmedMachinePublicationMember],
        forwarding_bindings: &[SandboxPortBinding],
    ) -> Result<Self, Error> {
        if command.step() != WorkloadTeardownStep::WithdrawPublication
            || command.mode() != WorkloadTeardownCommandMode::Execute
        {
            return Err(Error::conflict(
                "parent withdrawal fence requires an Execute withdrawal command",
            ));
        }
        let member_count = u32::try_from(members.len()).map_err(|_| {
            Error::ResourceExhausted("machine publication member count exceeds u32".to_owned())
        })?;
        let members_digest = member_batch_digest(members)?;
        let forwarding_binding_count = u32::try_from(forwarding_bindings.len()).map_err(|_| {
            Error::ResourceExhausted("machine forwarding binding count exceeds u32".to_owned())
        })?;
        let forwarding_bindings_digest = forwarding_binding_batch_digest(forwarding_bindings)?;
        let fence = Self {
            command: ConfirmedTeardownCommandFence::new(command, provider)?,
            member_count,
            members_digest,
            forwarding_binding_count,
            forwarding_bindings_digest,
        };
        fence.validate()?;
        Ok(fence)
    }

    fn validate(&self) -> Result<(), Error> {
        self.command.validate()?;
        if self.command.claim.attempt().step() != WorkloadTeardownStep::WithdrawPublication
            || self.command.mode != WorkloadTeardownCommandMode::Execute
        {
            return Err(Error::PreconditionFailed(
                "parent withdrawal fence has a crossed command".to_owned(),
            ));
        }
        Ok(())
    }

    pub(super) fn authenticate_candidate(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        provider: &ConfirmedTeardownProviderCommand,
        members: &[ConfirmedMachinePublicationMember],
        forwarding_bindings: &[SandboxPortBinding],
    ) -> Result<ConfirmedTeardownCommandFence, Error> {
        self.validate()?;
        if usize::try_from(self.member_count).map_err(|_| {
            Error::PreconditionFailed(
                "confirmed parent withdrawal member count is not representable".to_owned(),
            )
        })? != members.len()
            || self.members_digest != member_batch_digest(members)?
            || usize::try_from(self.forwarding_binding_count).map_err(|_| {
                Error::PreconditionFailed(
                    "confirmed forwarding binding count is not representable".to_owned(),
                )
            })? != forwarding_bindings.len()
            || self.forwarding_bindings_digest
                != forwarding_binding_batch_digest(forwarding_bindings)?
        {
            return Err(Error::conflict(
                "parent withdrawal candidate crosses the complete publication batch",
            ));
        }
        let candidate = ConfirmedTeardownCommandFence::new(command, provider)?;
        if command.step() != WorkloadTeardownStep::WithdrawPublication
            || command.mode() != WorkloadTeardownCommandMode::Execute
            || !self.command.same_provider_stream(&candidate)
        {
            return Err(Error::conflict(
                "parent withdrawal candidate crosses its durable command stream",
            ));
        }
        Ok(candidate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct ConfirmedParentWithdrawalEvidence {
    settled_by: ConfirmedTeardownCommandFence,
    forwarding_absence_digest: WorkloadOwnerEvidenceDigest,
    forwarding_binding_count: u32,
    retained_ports: ConfirmedParentPortBatchEvidence,
}

impl ConfirmedParentWithdrawalEvidence {
    pub(super) fn new(
        withdrawal: &ConfirmedParentWithdrawalFence,
        settled_by: ConfirmedTeardownCommandFence,
        forwarding: &[MachinePortForwardReceipt],
        ports: &[PortLeaseRecord],
        members: &[ConfirmedMachinePublicationMember],
        forwarding_bindings: &[SandboxPortBinding],
        authority: &MachineForwarderAuthority,
    ) -> Result<Self, Error> {
        let tenant_id = withdrawal.command.claim.attempt().key().tenant_id();
        let sandbox_id = withdrawal.command.execution_locator.execution_id().as_str();
        if forwarding.len() != forwarding_bindings.len()
            || forwarding.iter().any(|receipt| {
                !matches!(
                    receipt.outcome,
                    MachinePortForwardOutcome::Withdrawn
                        | MachinePortForwardOutcome::ExactAlreadyAbsent
                ) || &receipt.tenant_id != tenant_id
                    || receipt.sandbox_id.as_str() != sandbox_id
                    || receipt.provider_instance != *authority.provider_instance()
                    || receipt.provider_generation != authority.generation()
                    || !forwarding_bindings.contains(&receipt.binding)
            })
            || forwarding_bindings
                .iter()
                .any(|binding| !forwarding.iter().any(|receipt| receipt.binding == *binding))
        {
            return Err(Error::PreconditionFailed(
                "parent forwarding absence evidence crosses the complete publication batch"
                    .to_owned(),
            ));
        }
        let evidence = Self {
            settled_by,
            forwarding_absence_digest: forwarding_absence_digest(forwarding)?,
            forwarding_binding_count: u32::try_from(forwarding.len()).map_err(|_| {
                Error::ResourceExhausted("machine forwarding receipt count exceeds u32".to_owned())
            })?,
            retained_ports: ConfirmedParentPortBatchEvidence::retained(ports, members)?,
        };
        evidence.validate_for(withdrawal)?;
        Ok(evidence)
    }

    fn validate_for(&self, withdrawal: &ConfirmedParentWithdrawalFence) -> Result<(), Error> {
        self.settled_by.validate()?;
        if !withdrawal.command.same_provider_stream(&self.settled_by)
            || self.retained_ports.member_count != withdrawal.member_count
            || self.forwarding_binding_count != withdrawal.forwarding_binding_count
        {
            return Err(Error::PreconditionFailed(
                "withdrawn parent publication evidence crosses its command or member batch"
                    .to_owned(),
            ));
        }
        self.retained_ports
            .validate_retained(withdrawal.member_count)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct ConfirmedGuestReleaseEvidence {
    command: ConfirmedTeardownCommandFence,
    request_digest: MachineApiWorkloadTeardownRequestDigest,
    response_digest: WorkloadOwnerEvidenceDigest,
    network_released: WorkloadTeardownSuccessEvidence,
    guest_absence: MachineApiNetworkReleaseAbsenceEvidence,
}

impl ConfirmedGuestReleaseEvidence {
    pub(super) fn new(
        withdrawal: &ConfirmedParentWithdrawalFence,
        withdrawn: &ConfirmedParentWithdrawalEvidence,
        command: &ConfirmedWorkloadTeardownCommand,
        provider: &ConfirmedTeardownProviderCommand,
        request: &MachineApiWorkloadTeardownPhaseRequest,
        response: &MachineApiWorkloadTeardownPhaseResponse,
    ) -> Result<Self, Error> {
        response
            .validate_for_request(request)
            .map_err(|error| Error::PreconditionFailed(error.to_string()))?;
        if command.step() != WorkloadTeardownStep::ReleaseNetwork
            || request.command().command_id() != command.command_id()
        {
            return Err(Error::conflict(
                "guest release evidence crosses the confirmed ReleaseNetwork command",
            ));
        }
        let network_released = match response.observation() {
            MachineApiWorkloadTeardownObservation::Execute(
                MachineApiWorkloadTeardownExecuteObservation::Succeeded { evidence },
            )
            | MachineApiWorkloadTeardownObservation::Inspect(
                MachineApiWorkloadTeardownInspectObservation::Satisfied { evidence },
            ) => evidence.as_ref().clone(),
            _ => {
                return Err(Error::PreconditionFailed(
                    "guest release evidence requires exact network release success".to_owned(),
                ));
            }
        };
        let guest_absence = response.release_absence().ok_or_else(|| {
            Error::PreconditionFailed(
                "guest release response lacks independent provider and publication absence"
                    .to_owned(),
            )
        })?;
        let response_digest = domain_digest(RESPONSE_DOMAIN, response)?;
        let release = Self {
            command: ConfirmedTeardownCommandFence::new(command, provider)?,
            request_digest: request.request_digest(),
            response_digest,
            network_released,
            guest_absence,
        };
        release.validate_for(withdrawal, withdrawn)?;
        Ok(release)
    }

    fn validate_for(
        &self,
        withdrawal: &ConfirmedParentWithdrawalFence,
        withdrawn: &ConfirmedParentWithdrawalEvidence,
    ) -> Result<(), Error> {
        self.command.validate()?;
        withdrawn.validate_for(withdrawal)?;
        let settled_receipt = self
            .command
            .prior_receipt_prefix
            .receipt_for(WorkloadTeardownStep::WithdrawPublication)
            .ok_or_else(|| {
                Error::PreconditionFailed(
                    "guest release lacks the settled parent withdrawal receipt".to_owned(),
                )
            })?;
        if self.command.claim.attempt().step() != WorkloadTeardownStep::ReleaseNetwork
            || !withdrawal.command.same_workload_lifecycle(&self.command)
            || settled_receipt.claim() != &withdrawn.settled_by.claim
            || !self.network_released.matches_step_and_subjects(
                WorkloadTeardownStep::ReleaseNetwork,
                self.command.claim.attempt().subjects(),
            )
        {
            return Err(Error::PreconditionFailed(
                "guest release evidence crosses the retained workload lifecycle".to_owned(),
            ));
        }
        Ok(())
    }

    pub(super) fn authenticates_recovery(&self, candidate: &Self) -> bool {
        self.command.claim == candidate.command.claim
            && self.command.source == candidate.command.source
            && self.command.compiled_network_plan == candidate.command.compiled_network_plan
            && self.command.execution_locator == candidate.command.execution_locator
            && self.command.prior_receipt_prefix == candidate.command.prior_receipt_prefix
            && self.command.provider_claim == candidate.command.provider_claim
            && self.network_released == candidate.network_released
            && self.guest_absence == candidate.guest_absence
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConfirmedParentPortBatchState {
    Empty,
    Retained,
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct ConfirmedParentPortBatchEvidence {
    state: ConfirmedParentPortBatchState,
    member_count: u32,
    records_digest: WorkloadOwnerEvidenceDigest,
}

impl ConfirmedParentPortBatchEvidence {
    fn retained(
        records: &[PortLeaseRecord],
        members: &[ConfirmedMachinePublicationMember],
    ) -> Result<Self, Error> {
        authenticate_port_members(records, members)?;
        let state = if records.is_empty() {
            ConfirmedParentPortBatchState::Empty
        } else if records.iter().all(|record| {
            record.phase() == PortLeasePhase::CleanupPending
                && record.failure().is_none()
                && record.active_lifetime().is_none()
                && retained_record_matches_member(record, members)
        }) {
            ConfirmedParentPortBatchState::Retained
        } else {
            return Err(Error::PreconditionFailed(
                "parent publication ports are not a complete non-bindable retained batch"
                    .to_owned(),
            ));
        };
        Self::from_records(state, records)
    }

    pub(super) fn released(
        records: &[PortLeaseRecord],
        members: &[ConfirmedMachinePublicationMember],
    ) -> Result<Self, Error> {
        authenticate_port_members(records, members)?;
        let state = if records.is_empty() {
            ConfirmedParentPortBatchState::Empty
        } else if records.iter().all(|record| {
            record.phase() == PortLeasePhase::Released
                && record.active_lifetime().is_none()
                && released_record_matches_member(record, members)
        }) {
            ConfirmedParentPortBatchState::Released
        } else {
            return Err(Error::PreconditionFailed(
                "parent publication ports lack complete terminal release evidence".to_owned(),
            ));
        };
        Self::from_records(state, records)
    }

    fn from_records(
        state: ConfirmedParentPortBatchState,
        records: &[PortLeaseRecord],
    ) -> Result<Self, Error> {
        Ok(Self {
            state,
            member_count: u32::try_from(records.len()).map_err(|_| {
                Error::ResourceExhausted("parent port batch exceeds u32".to_owned())
            })?,
            records_digest: port_batch_digest(records)?,
        })
    }

    fn validate_retained(&self, expected: u32) -> Result<(), Error> {
        if self.member_count != expected
            || (expected == 0) != (self.state == ConfirmedParentPortBatchState::Empty)
            || (expected > 0) != (self.state == ConfirmedParentPortBatchState::Retained)
        {
            return Err(Error::PreconditionFailed(
                "retained parent port evidence has crossed state or membership".to_owned(),
            ));
        }
        Ok(())
    }

    fn validate_released(&self, expected: u32) -> Result<(), Error> {
        if self.member_count != expected
            || (expected == 0) != (self.state == ConfirmedParentPortBatchState::Empty)
            || (expected > 0) != (self.state == ConfirmedParentPortBatchState::Released)
        {
            return Err(Error::PreconditionFailed(
                "released parent port evidence has crossed state or membership".to_owned(),
            ));
        }
        Ok(())
    }
}

fn provider_operation(step: WorkloadTeardownStep) -> ProviderCommandOperation {
    match step {
        WorkloadTeardownStep::WithdrawPublication => {
            ProviderCommandOperation::WithdrawFinalPublication
        }
        WorkloadTeardownStep::DrainExecution => ProviderCommandOperation::DrainExecution,
        WorkloadTeardownStep::StopExecution => ProviderCommandOperation::StopExecution,
        WorkloadTeardownStep::DetachNetwork => ProviderCommandOperation::DetachNetwork,
        WorkloadTeardownStep::ReleaseNetwork => ProviderCommandOperation::ReleaseNetwork,
    }
}

fn required_prior_receipt_steps(step: WorkloadTeardownStep) -> &'static [WorkloadTeardownStep] {
    match step {
        WorkloadTeardownStep::WithdrawPublication => &[],
        WorkloadTeardownStep::DrainExecution => &[WorkloadTeardownStep::WithdrawPublication],
        WorkloadTeardownStep::StopExecution => &[
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownStep::DrainExecution,
        ],
        WorkloadTeardownStep::DetachNetwork => &[
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownStep::DrainExecution,
            WorkloadTeardownStep::StopExecution,
        ],
        WorkloadTeardownStep::ReleaseNetwork => &[
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownStep::DrainExecution,
            WorkloadTeardownStep::StopExecution,
            WorkloadTeardownStep::DetachNetwork,
        ],
    }
}

fn member_batch_digest(
    members: &[ConfirmedMachinePublicationMember],
) -> Result<WorkloadOwnerEvidenceDigest, Error> {
    let stable = members
        .iter()
        .map(|member| {
            (
                &member.listener_id,
                &member.binding,
                &member.request,
                &member.expected_binding,
            )
        })
        .collect::<Vec<_>>();
    domain_digest(MEMBER_BATCH_DOMAIN, &stable)
}

fn forwarding_binding_batch_digest(
    bindings: &[SandboxPortBinding],
) -> Result<WorkloadOwnerEvidenceDigest, Error> {
    let mut stable = bindings.to_vec();
    stable.sort_by(|left, right| {
        (
            left.host_port,
            left.guest_port,
            left.name.as_str(),
            endpoint_protocol_order(left.protocol),
        )
            .cmp(&(
                right.host_port,
                right.guest_port,
                right.name.as_str(),
                endpoint_protocol_order(right.protocol),
            ))
    });
    domain_digest(FORWARDING_BINDING_BATCH_DOMAIN, &stable)
}

const fn endpoint_protocol_order(protocol: nimbus_network::EndpointProtocol) -> u8 {
    match protocol {
        nimbus_network::EndpointProtocol::Tcp => 0,
        nimbus_network::EndpointProtocol::Http => 1,
        nimbus_network::EndpointProtocol::Https => 2,
    }
}

fn forwarding_absence_digest(
    receipts: &[MachinePortForwardReceipt],
) -> Result<WorkloadOwnerEvidenceDigest, Error> {
    let mut stable = receipts.to_vec();
    stable.sort_by(|left, right| {
        (
            left.binding.host_port,
            left.binding.guest_port,
            left.binding.name.as_str(),
        )
            .cmp(&(
                right.binding.host_port,
                right.binding.guest_port,
                right.binding.name.as_str(),
            ))
    });
    domain_digest(FORWARDING_ABSENCE_DOMAIN, &stable)
}

fn port_batch_digest(records: &[PortLeaseRecord]) -> Result<WorkloadOwnerEvidenceDigest, Error> {
    let mut stable = records.to_vec();
    stable.sort_by(|left, right| left.request().lease_id().cmp(right.request().lease_id()));
    domain_digest(PORT_BATCH_DOMAIN, &stable)
}

fn authenticate_port_members(
    records: &[PortLeaseRecord],
    members: &[ConfirmedMachinePublicationMember],
) -> Result<(), Error> {
    if records.len() != members.len()
        || members.iter().any(|member| {
            !records
                .iter()
                .any(|record| record.request() == &member.request)
        })
    {
        return Err(Error::PreconditionFailed(
            "parent port evidence crosses the complete publication member batch".to_owned(),
        ));
    }
    Ok(())
}

fn retained_record_matches_member(
    record: &PortLeaseRecord,
    members: &[ConfirmedMachinePublicationMember],
) -> bool {
    members
        .iter()
        .find(|member| record.request() == member.request())
        .is_some_and(|member| match record.binding() {
            Some(binding) => {
                binding == member.expected_binding()
                    && record.adoption_claim() == Some(member.bind_claim())
                    && record.bind_claim().is_none()
            }
            None => {
                record.adoption_claim().is_none()
                    && record.bind_claim() == Some(member.bind_claim())
            }
        })
}

fn released_record_matches_member(
    record: &PortLeaseRecord,
    members: &[ConfirmedMachinePublicationMember],
) -> bool {
    members
        .iter()
        .find(|member| record.request() == member.request())
        .is_some_and(|member| match record.binding() {
            Some(binding) => {
                binding == member.expected_binding()
                    && record.adoption_claim() == Some(member.bind_claim())
                    && record.bind_claim().is_none()
            }
            None => record.adoption_claim().is_none() && record.bind_claim().is_none(),
        })
}

fn domain_digest(
    domain: &[u8],
    value: &impl Serialize,
) -> Result<WorkloadOwnerEvidenceDigest, Error> {
    let encoded = serde_json::to_vec(value).map_err(|error| {
        Error::Internal(format!(
            "failed to encode machine retirement evidence: {error}"
        ))
    })?;
    let mut preimage = Vec::with_capacity(domain.len() + encoded.len());
    preimage.extend_from_slice(domain);
    preimage.extend_from_slice(&encoded);
    Ok(WorkloadOwnerEvidenceDigest::sha256(preimage))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_phase_projection_is_strict_and_terminal() {
        assert_eq!(
            ConfirmedMachinePublicationRetirementProgress::Active.phase(),
            ConfirmedMachinePublicationRetirementPhase::Active
        );
        assert!(ConfirmedMachinePublicationRetirementPhase::Released.is_released());
        assert!(
            ConfirmedMachinePublicationRetirementProgress::LegacyReleased
                .validate()
                .is_ok()
        );
    }
}
