//! Strict durable progress and compound evidence for exact attachment teardown.

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAttachmentId, NetworkAttachmentSegmentAssociation, NetworkCapabilitySourceDigest,
    NetworkLeaseEpoch, NetworkPlanDigest, NetworkPlanId, NetworkProviderId,
    NetworkResourceGeneration,
};
use serde::{Deserialize, Serialize};

use crate::{
    ProviderCommandClaim, ProviderCommandObservation, ProviderCommandObservationKind,
    ProviderCommandOperation, SandboxExecutionAttemptId, SandboxId, SandboxNetworkTeardownCommand,
    SandboxNetworkTeardownOperation,
};

use crate::error::{Result, SandboxError};

const DETACHED_PROOF_SCHEMA_VERSION: u32 = 1;

/// Durable boundaries for provider detach while reusable authority stays held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HostManagedAttachmentDetachPhase {
    NotStarted,
    AttachmentDeleting,
    SegmentQuarantined,
    PepStopMayExist,
    PepRetained,
    ListenerStopMayExist,
    ProviderDeleteMayExist,
    ProviderAbsent,
    NamespaceRemoveMayExist,
    NamespaceAbsent,
    ListenersRetained,
    Detached,
}

impl HostManagedAttachmentDetachPhase {
    const fn ordinal(self) -> u8 {
        self as u8
    }
}

/// Durable boundaries for release after an exact detached proof exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HostManagedAttachmentReleasePhase {
    NotStarted,
    ReleaseAuthenticated,
    PepReleaseMayExist,
    PepReleased,
    ListenerReleaseMayExist,
    ListenersReleased,
    IpamReleaseMayExist,
    IpamReleased,
    SegmentReleaseMayExist,
    SegmentReleased,
    AttachmentReleaseMayExist,
    Released,
}

impl HostManagedAttachmentReleasePhase {
    const fn ordinal(self) -> u8 {
        self as u8
    }
}

/// Test-only abrupt-process boundary after one exact checkpoint is durable.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostManagedAttachmentTeardownCheckpoint {
    Detach(HostManagedAttachmentDetachPhase),
    Release(HostManagedAttachmentReleasePhase),
}

/// Test-only probe that terminates a subprocess after one durable checkpoint.
///
/// The abrupt exit does not unwind or run destructors. A separate process must
/// reopen the durable roots and prove that recovery needs no in-memory state.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct HostManagedAttachmentCheckpointTestProbe {
    checkpoint: HostManagedAttachmentTeardownCheckpoint,
    exit_code: i32,
}

#[cfg(test)]
impl HostManagedAttachmentCheckpointTestProbe {
    pub(crate) const fn exit_after(
        checkpoint: HostManagedAttachmentTeardownCheckpoint,
        exit_code: i32,
    ) -> Self {
        Self {
            checkpoint,
            exit_code,
        }
    }

    pub(crate) fn exit_if_reached(&self, state: &HostManagedAttachmentTeardownState) {
        let reached = match self.checkpoint {
            HostManagedAttachmentTeardownCheckpoint::Detach(phase) => state.detach_phase() == phase,
            HostManagedAttachmentTeardownCheckpoint::Release(phase) => {
                state.release_phase() == phase
            }
        };
        if reached {
            std::process::exit(self.exit_code);
        }
    }
}

/// State-owned result of authenticating one exact teardown command replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostManagedAttachmentCommandInspection {
    /// The exact command already owns terminal state and can replay success.
    ExactTerminalSuccess,
    /// The exact current command can continue from its durable checkpoint.
    ExactCurrentPartial,
    /// An authenticated adjacent retry replaced only the operation's claim.
    AuthorizedImmediatePredecessor,
}

/// Stable state classification for a rejected teardown command replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostManagedAttachmentCommandInspectionError {
    /// The command or observation belongs to another command attempt.
    Crossed,
    /// The candidate epoch is stale, skipped, overflowed, or terminally fenced.
    EpochInvalid,
    /// Durable state or retry authorization is structurally inconsistent.
    Corrupt,
}

/// Exact retained-detach evidence persisted in the owning backend manifest.
///
/// Digests bind read-only snapshots of provider-local authorities. Those
/// snapshots are evidence only; their concept owners remain authoritative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct HostManagedAttachmentDetachedProof {
    schema_version: u32,
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    execution_attempt_id: SandboxExecutionAttemptId,
    attachment_id: NetworkAttachmentId,
    network_plan_id: NetworkPlanId,
    network_generation: NetworkResourceGeneration,
    network_plan_digest: NetworkPlanDigest,
    lease_epoch: NetworkLeaseEpoch,
    association: NetworkAttachmentSegmentAssociation,
    selected_provider_id: NetworkProviderId,
    provider_source_digest: NetworkCapabilitySourceDigest,
    stable_handle_sha256: String,
    provider_delete_evidence_sha256: String,
    namespace_absence_evidence_sha256: String,
    pep_retained_evidence_sha256: String,
    listener_retained_evidence_sha256: String,
    ipam_retained_evidence_sha256: String,
    segment_quarantine_evidence_sha256: String,
    attachment_retained_evidence_sha256: String,
    detach_claim: ProviderCommandClaim,
}

pub(super) struct HostManagedAttachmentDetachedProofInput {
    pub(super) command: SandboxNetworkTeardownCommand,
    pub(super) association: NetworkAttachmentSegmentAssociation,
    pub(super) selected_provider_id: NetworkProviderId,
    pub(super) stable_handle_sha256: String,
    pub(super) provider_delete_evidence_sha256: String,
    pub(super) namespace_absence_evidence_sha256: String,
    pub(super) pep_retained_evidence_sha256: String,
    pub(super) listener_retained_evidence_sha256: String,
    pub(super) ipam_retained_evidence_sha256: String,
    pub(super) segment_quarantine_evidence_sha256: String,
    pub(super) attachment_retained_evidence_sha256: String,
}

pub(super) struct HostManagedAttachmentDetachedEvidence<'a> {
    pub(super) stable_handle_sha256: &'a str,
    pub(super) provider_delete_evidence_sha256: &'a str,
    pub(super) namespace_absence_evidence_sha256: &'a str,
    pub(super) pep_retained_evidence_sha256: &'a str,
    pub(super) listener_retained_evidence_sha256: &'a str,
    pub(super) ipam_retained_evidence_sha256: &'a str,
    pub(super) segment_quarantine_evidence_sha256: &'a str,
    pub(super) attachment_retained_evidence_sha256: &'a str,
}

impl HostManagedAttachmentDetachedProof {
    pub(super) fn new(input: HostManagedAttachmentDetachedProofInput) -> Result<Self> {
        require_sha256("stable provider handle", &input.stable_handle_sha256)?;
        for (label, digest) in [
            ("provider delete", &input.provider_delete_evidence_sha256),
            (
                "namespace absence",
                &input.namespace_absence_evidence_sha256,
            ),
            ("retained PEP", &input.pep_retained_evidence_sha256),
            (
                "retained published listeners",
                &input.listener_retained_evidence_sha256,
            ),
            ("retained IPAM", &input.ipam_retained_evidence_sha256),
            (
                "quarantined segment",
                &input.segment_quarantine_evidence_sha256,
            ),
            (
                "retained attachment",
                &input.attachment_retained_evidence_sha256,
            ),
        ] {
            require_sha256(label, digest)?;
        }
        if input.command.operation() != SandboxNetworkTeardownOperation::Detach {
            return Err(state_error(
                "compound detached proof requires a DetachNetwork command",
            ));
        }
        if input.selected_provider_id != input.command.provider_id() {
            return Err(state_error(
                "compound detached proof crossed its selected provider",
            ));
        }
        let plan = input.command.network_plan();
        Ok(Self {
            schema_version: DETACHED_PROOF_SCHEMA_VERSION,
            tenant_id: input.command.tenant_id().clone(),
            sandbox_id: input.command.sandbox_id().clone(),
            execution_attempt_id: input.command.execution_attempt_id().clone(),
            attachment_id: input.command.attachment_id().clone(),
            network_plan_id: plan.plan_id().clone(),
            network_generation: plan.generation(),
            network_plan_digest: plan.digest(),
            lease_epoch: input.association.lease_epoch(),
            association: input.association,
            selected_provider_id: input.selected_provider_id,
            provider_source_digest: input.command.provider_source_digest(),
            stable_handle_sha256: input.stable_handle_sha256,
            provider_delete_evidence_sha256: input.provider_delete_evidence_sha256,
            namespace_absence_evidence_sha256: input.namespace_absence_evidence_sha256,
            pep_retained_evidence_sha256: input.pep_retained_evidence_sha256,
            listener_retained_evidence_sha256: input.listener_retained_evidence_sha256,
            ipam_retained_evidence_sha256: input.ipam_retained_evidence_sha256,
            segment_quarantine_evidence_sha256: input.segment_quarantine_evidence_sha256,
            attachment_retained_evidence_sha256: input.attachment_retained_evidence_sha256,
            detach_claim: input.command.provider_claim().clone(),
        })
    }

    pub(crate) fn detach_claim(&self) -> &ProviderCommandClaim {
        &self.detach_claim
    }

    pub(crate) fn association(&self) -> &NetworkAttachmentSegmentAssociation {
        &self.association
    }

    pub(crate) fn selected_provider_id(&self) -> &NetworkProviderId {
        &self.selected_provider_id
    }

    pub(crate) fn lease_epoch(&self) -> NetworkLeaseEpoch {
        self.lease_epoch
    }

    pub(super) fn require_current_evidence(
        &self,
        evidence: HostManagedAttachmentDetachedEvidence<'_>,
    ) -> Result<()> {
        if self.stable_handle_sha256 != evidence.stable_handle_sha256
            || self.provider_delete_evidence_sha256 != evidence.provider_delete_evidence_sha256
            || self.namespace_absence_evidence_sha256 != evidence.namespace_absence_evidence_sha256
            || self.pep_retained_evidence_sha256 != evidence.pep_retained_evidence_sha256
            || self.listener_retained_evidence_sha256 != evidence.listener_retained_evidence_sha256
            || self.ipam_retained_evidence_sha256 != evidence.ipam_retained_evidence_sha256
            || self.segment_quarantine_evidence_sha256
                != evidence.segment_quarantine_evidence_sha256
            || self.attachment_retained_evidence_sha256
                != evidence.attachment_retained_evidence_sha256
        {
            return Err(state_error(
                "current provider-local authority crossed the compound detached proof",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_detach_command(
        &self,
        command: &SandboxNetworkTeardownCommand,
    ) -> Result<()> {
        if command.operation() != SandboxNetworkTeardownOperation::Detach
            || self.detach_claim != *command.provider_claim()
        {
            return Err(state_error(
                "compound detached proof crossed its exact DetachNetwork claim",
            ));
        }
        self.validate_identity(command)
    }

    pub(crate) fn validate_release_command(
        &self,
        command: &SandboxNetworkTeardownCommand,
    ) -> Result<()> {
        if command.operation() != SandboxNetworkTeardownOperation::Release
            || self.detach_claim.operation() != ProviderCommandOperation::DetachNetwork
            || self.detach_claim.authority_id() != command.provider_claim().authority_id()
            || self.detach_claim.effect_subject() != command.provider_claim().effect_subject()
            || self.detach_claim.workload_generation()
                != command.provider_claim().workload_generation()
            || self.detach_claim.desired_digest() != command.provider_claim().desired_digest()
            || self.detach_claim.source_digest() != command.provider_claim().source_digest()
            || self.detach_claim.network_plan_digest()
                != command.provider_claim().network_plan_digest()
            || self.detach_claim.provider_target_digest()
                != command.provider_claim().provider_target_digest()
        {
            return Err(state_error(
                "ReleaseNetwork command crossed the retained DetachNetwork proof",
            ));
        }
        self.validate_identity(command)
    }

    fn validate_identity(&self, command: &SandboxNetworkTeardownCommand) -> Result<()> {
        let plan = command.network_plan();
        self.validate_integrity()?;
        if self.tenant_id != *command.tenant_id()
            || self.sandbox_id != *command.sandbox_id()
            || self.execution_attempt_id != *command.execution_attempt_id()
            || self.attachment_id != *command.attachment_id()
            || self.network_plan_id != *plan.plan_id()
            || self.network_generation != plan.generation()
            || self.network_plan_digest != plan.digest()
            || self.lease_epoch != self.association.lease_epoch()
            || self.selected_provider_id != command.provider_id()
            || self.provider_source_digest != command.provider_source_digest()
        {
            return Err(state_error(
                "compound detached proof crossed its attachment identity or provider fences",
            ));
        }
        Ok(())
    }

    fn validate_integrity(&self) -> Result<()> {
        if self.schema_version != DETACHED_PROOF_SCHEMA_VERSION
            || self.lease_epoch != self.association.lease_epoch()
            || self.detach_claim.operation() != ProviderCommandOperation::DetachNetwork
        {
            return Err(state_error(
                "compound detached proof has corrupt schema, association, or claim state",
            ));
        }
        for (label, digest) in [
            ("stable provider handle", &self.stable_handle_sha256),
            ("provider delete", &self.provider_delete_evidence_sha256),
            ("namespace absence", &self.namespace_absence_evidence_sha256),
            ("retained PEP", &self.pep_retained_evidence_sha256),
            (
                "retained published listeners",
                &self.listener_retained_evidence_sha256,
            ),
            ("retained IPAM", &self.ipam_retained_evidence_sha256),
            (
                "quarantined segment",
                &self.segment_quarantine_evidence_sha256,
            ),
            (
                "retained attachment",
                &self.attachment_retained_evidence_sha256,
            ),
        ] {
            require_sha256(label, digest)?;
        }
        Ok(())
    }
}

/// Backend-manifest effect progress. This is not a command-result journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct HostManagedAttachmentTeardownState {
    detach_claim: Option<ProviderCommandClaim>,
    detach_phase: HostManagedAttachmentDetachPhase,
    detached_proof: Option<HostManagedAttachmentDetachedProof>,
    release_claim: Option<ProviderCommandClaim>,
    release_phase: HostManagedAttachmentReleasePhase,
}

impl HostManagedAttachmentTeardownState {
    pub(crate) const fn initial() -> Self {
        Self {
            detach_claim: None,
            detach_phase: HostManagedAttachmentDetachPhase::NotStarted,
            detached_proof: None,
            release_claim: None,
            release_phase: HostManagedAttachmentReleasePhase::NotStarted,
        }
    }

    pub(crate) const fn detach_phase(&self) -> HostManagedAttachmentDetachPhase {
        self.detach_phase
    }

    pub(crate) const fn release_phase(&self) -> HostManagedAttachmentReleasePhase {
        self.release_phase
    }

    #[cfg(test)]
    pub(crate) fn detached_proof(&self) -> Option<&HostManagedAttachmentDetachedProof> {
        self.detached_proof.as_ref()
    }

    pub(crate) fn record_detach_phase(
        &mut self,
        claim: &ProviderCommandClaim,
        phase: HostManagedAttachmentDetachPhase,
    ) -> Result<bool> {
        if claim.operation() != ProviderCommandOperation::DetachNetwork
            || phase == HostManagedAttachmentDetachPhase::Detached
        {
            return Err(state_error(
                "detach effect progress requires a nonterminal DetachNetwork checkpoint",
            ));
        }
        require_same_or_unset_claim(&self.detach_claim, claim, "detach")?;
        let changed = advance_phase(self.detach_phase.ordinal(), phase.ordinal(), "detach")?;
        if changed {
            self.detach_claim = Some(claim.clone());
            self.detach_phase = phase;
        }
        Ok(changed)
    }

    pub(crate) fn record_detached_for_command(
        &mut self,
        command: &SandboxNetworkTeardownCommand,
        proof: HostManagedAttachmentDetachedProof,
    ) -> Result<bool> {
        proof.validate_detach_command(command)?;
        require_same_or_unset_claim(&self.detach_claim, command.provider_claim(), "detach")?;
        let changed = advance_phase(
            self.detach_phase.ordinal(),
            HostManagedAttachmentDetachPhase::Detached.ordinal(),
            "detach",
        )?;
        if changed {
            self.detach_claim = Some(command.provider_claim().clone());
            self.detach_phase = HostManagedAttachmentDetachPhase::Detached;
            self.detached_proof = Some(proof);
        } else if self.detached_proof.as_ref() != Some(&proof) {
            return Err(state_error(
                "detached proof replay crossed the durable compound proof",
            ));
        }
        Ok(changed)
    }

    pub(crate) fn require_detached_for_release(
        &self,
        command: &SandboxNetworkTeardownCommand,
    ) -> Result<&HostManagedAttachmentDetachedProof> {
        if self.detach_phase != HostManagedAttachmentDetachPhase::Detached {
            return Err(state_error(
                "ReleaseNetwork requires completed retained detach progress",
            ));
        }
        let proof = self
            .detached_proof
            .as_ref()
            .ok_or_else(|| state_error("detached phase omitted its compound proof"))?;
        proof.validate_release_command(command)?;
        Ok(proof)
    }

    pub(crate) fn record_release_phase(
        &mut self,
        command: &SandboxNetworkTeardownCommand,
        phase: HostManagedAttachmentReleasePhase,
    ) -> Result<bool> {
        if command.operation() != SandboxNetworkTeardownOperation::Release
            || phase == HostManagedAttachmentReleasePhase::NotStarted
        {
            return Err(state_error(
                "release progress requires a noninitial ReleaseNetwork checkpoint",
            ));
        }
        self.require_detached_for_release(command)?;
        require_same_or_unset_claim(&self.release_claim, command.provider_claim(), "release")?;
        let changed = advance_phase(self.release_phase.ordinal(), phase.ordinal(), "release")?;
        if changed {
            self.release_claim = Some(command.provider_claim().clone());
            self.release_phase = phase;
        }
        Ok(changed)
    }

    /// Authenticate an exact current command and rebase only an authorized
    /// adjacent partial claim.
    ///
    /// The caller must supply the journal's current observation while holding
    /// the provider-command stream lock. Exact replays do not change this
    /// state. A retry changes only the selected detach or release claim after
    /// the current claimed observation proves the stored claim in its lineage.
    pub(crate) fn inspect_and_rebase_command(
        &mut self,
        command: &SandboxNetworkTeardownCommand,
        current: &ProviderCommandObservation,
    ) -> std::result::Result<
        HostManagedAttachmentCommandInspection,
        HostManagedAttachmentCommandInspectionError,
    > {
        self.validate()
            .map_err(|_| HostManagedAttachmentCommandInspectionError::Corrupt)?;
        let command_claim = command.provider_claim();
        if current.claim() != command_claim {
            return Err(classify_claim_mismatch(current.claim(), command_claim));
        }
        if !matches!(
            current.kind(),
            ProviderCommandObservationKind::Claimed
                | ProviderCommandObservationKind::InProgress
                | ProviderCommandObservationKind::Ambiguous
        ) {
            return Err(HostManagedAttachmentCommandInspectionError::Corrupt);
        }

        let (stored_claim, terminal) = match command.operation() {
            SandboxNetworkTeardownOperation::Detach => (
                &self.detach_claim,
                self.detach_phase == HostManagedAttachmentDetachPhase::Detached,
            ),
            SandboxNetworkTeardownOperation::Release => {
                if self.detach_phase != HostManagedAttachmentDetachPhase::Detached {
                    return Err(HostManagedAttachmentCommandInspectionError::Corrupt);
                }
                self.detached_proof
                    .as_ref()
                    .ok_or(HostManagedAttachmentCommandInspectionError::Corrupt)?
                    .validate_release_command(command)
                    .map_err(|_| HostManagedAttachmentCommandInspectionError::Crossed)?;
                (
                    &self.release_claim,
                    self.release_phase == HostManagedAttachmentReleasePhase::Released,
                )
            }
        };

        let Some(stored_claim_value) = stored_claim.as_ref() else {
            return Ok(HostManagedAttachmentCommandInspection::ExactCurrentPartial);
        };
        if stored_claim_value == command_claim {
            if command.operation() == SandboxNetworkTeardownOperation::Detach && terminal {
                self.detached_proof
                    .as_ref()
                    .ok_or(HostManagedAttachmentCommandInspectionError::Corrupt)?
                    .validate_detach_command(command)
                    .map_err(|_| HostManagedAttachmentCommandInspectionError::Crossed)?;
            }
            return Ok(if terminal {
                HostManagedAttachmentCommandInspection::ExactTerminalSuccess
            } else {
                HostManagedAttachmentCommandInspection::ExactCurrentPartial
            });
        }
        if !same_command_attempt(stored_claim_value, command_claim) {
            return Err(HostManagedAttachmentCommandInspectionError::Crossed);
        }
        if terminal
            || stored_claim_value.dispatch_epoch().checked_add(1)
                != Some(command_claim.dispatch_epoch())
        {
            return Err(HostManagedAttachmentCommandInspectionError::EpochInvalid);
        }
        if current.kind() != ProviderCommandObservationKind::Claimed
            || !current.authenticates_retry_progress(stored_claim_value)
        {
            return Err(HostManagedAttachmentCommandInspectionError::Corrupt);
        }

        match command.operation() {
            SandboxNetworkTeardownOperation::Detach => {
                self.detach_claim = Some(command_claim.clone());
            }
            SandboxNetworkTeardownOperation::Release => {
                self.release_claim = Some(command_claim.clone());
            }
        }
        Ok(HostManagedAttachmentCommandInspection::AuthorizedImmediatePredecessor)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        match (
            self.detach_phase,
            self.detach_claim.as_ref(),
            self.detached_proof.as_ref(),
        ) {
            (HostManagedAttachmentDetachPhase::NotStarted, None, None) => {}
            (HostManagedAttachmentDetachPhase::Detached, Some(claim), Some(proof))
                if claim == proof.detach_claim() => {}
            (HostManagedAttachmentDetachPhase::NotStarted, _, _)
            | (HostManagedAttachmentDetachPhase::Detached, _, _)
            | (_, None, _)
            | (_, _, Some(_)) => {
                return Err(state_error(
                    "detach progress has an invalid claim/proof shape",
                ));
            }
            (_, Some(claim), None)
                if claim.operation() == ProviderCommandOperation::DetachNetwork => {}
            _ => {
                return Err(state_error(
                    "detach progress carries a crossed command claim",
                ));
            }
        }
        if let Some(proof) = self.detached_proof.as_ref() {
            proof.validate_integrity()?;
        }
        match (self.release_phase, self.release_claim.as_ref()) {
            (HostManagedAttachmentReleasePhase::NotStarted, None) => {}
            (HostManagedAttachmentReleasePhase::NotStarted, Some(_)) | (_, None) => {
                return Err(state_error("release progress has an invalid claim shape"));
            }
            (_, Some(claim)) if claim.operation() == ProviderCommandOperation::ReleaseNetwork => {}
            _ => {
                return Err(state_error(
                    "release progress carries a crossed command claim",
                ));
            }
        }
        if self.release_phase != HostManagedAttachmentReleasePhase::NotStarted
            && self.detach_phase != HostManagedAttachmentDetachPhase::Detached
        {
            return Err(state_error(
                "release progress exists without a completed retained detach",
            ));
        }
        Ok(())
    }
}

fn classify_claim_mismatch(
    current: &ProviderCommandClaim,
    command: &ProviderCommandClaim,
) -> HostManagedAttachmentCommandInspectionError {
    if same_command_attempt(current, command) {
        HostManagedAttachmentCommandInspectionError::EpochInvalid
    } else {
        HostManagedAttachmentCommandInspectionError::Crossed
    }
}

fn same_command_attempt(left: &ProviderCommandClaim, right: &ProviderCommandClaim) -> bool {
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
}

impl Default for HostManagedAttachmentTeardownState {
    fn default() -> Self {
        Self::initial()
    }
}

fn advance_phase(current: u8, requested: u8, operation: &str) -> Result<bool> {
    if requested <= current {
        return Ok(false);
    }
    if requested != current + 1 {
        return Err(state_error(&format!(
            "{operation} progress cannot skip from checkpoint {current} to {requested}"
        )));
    }
    Ok(true)
}

fn require_same_or_unset_claim(
    current: &Option<ProviderCommandClaim>,
    candidate: &ProviderCommandClaim,
    operation: &str,
) -> Result<()> {
    if current.as_ref().is_none_or(|current| current == candidate) {
        return Ok(());
    }
    Err(state_error(&format!(
        "{operation} progress crossed its exact provider command claim"
    )))
}

fn require_sha256(label: &str, digest: &str) -> Result<()> {
    if digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Ok(());
    }
    Err(state_error(&format!(
        "{label} evidence must be canonical SHA-256"
    )))
}

fn state_error(message: &str) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("host-managed attachment teardown state rejected operation: {message}"),
    }
}

#[cfg(test)]
#[path = "state/tests.rs"]
mod tests;
