//! Least-authority bridge from exact startup quarantine to backend cleanup.
//!
//! The generic startup owner selects and durably fences a candidate before it
//! offers this immutable subject to a concrete Container or Krun adapter. The
//! subject carries identity, not mutation authority. Backend manifests remain
//! cleanup context and cannot mint a candidate or bypass quarantine.

use nimbus_core::TenantId;
use nimbus_network::{
    DurableNetworkAttachmentState, NetworkAttachmentId, NetworkAttachmentReservationState,
    NetworkAttachmentSegmentAssociation, NetworkReservationClaim, NetworkSegmentId,
};

use super::attachment_lifecycle::{
    AttachmentBackendKind, oci_attachment_provider_handle_for_identity,
};
use super::dto::NetavarkProviderOperation;
use super::ipam::{OciAttachmentProviderEvidence, OciIpamEvidenceLifecycle};
use super::layout::OciNetworkConfig;
use super::orphan_evidence::{
    OciAllocatorEvidenceSource, OciArtifactKind, OciArtifactObservationState,
    OciOrphanEvidenceCandidate, OciOrphanQuarantineReason,
};
use super::provider_locator::OciAttachmentProviderKind;
use crate::error::Result;
use crate::instance::SandboxId;

/// Immutable identity offered only after exact quarantine is durable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OciOrphanCleanupSubject {
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    attachment_id: NetworkAttachmentId,
    segment_id: NetworkSegmentId,
    reservation_claim: NetworkReservationClaim,
    desired: Option<DurableNetworkAttachmentState>,
    backend: AttachmentBackendKind,
    kind: OciOrphanCleanupKind,
}

/// Closed cleanup work selected only from one already fenced evidence row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OciOrphanCleanupKind {
    NeverEffected,
    Effectful,
    TerminalPublication,
}

impl OciOrphanCleanupSubject {
    pub(crate) fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub(crate) fn sandbox_id(&self) -> &SandboxId {
        &self.sandbox_id
    }

    pub(crate) fn attachment_id(&self) -> &NetworkAttachmentId {
        &self.attachment_id
    }

    pub(crate) fn segment_id(&self) -> &NetworkSegmentId {
        &self.segment_id
    }

    pub(crate) fn reservation_claim(&self) -> &NetworkReservationClaim {
        &self.reservation_claim
    }

    pub(crate) fn desired(&self) -> Option<&DurableNetworkAttachmentState> {
        self.desired.as_ref()
    }

    pub(crate) fn backend(&self) -> AttachmentBackendKind {
        self.backend
    }

    pub(crate) fn kind(&self) -> OciOrphanCleanupKind {
        self.kind
    }

    pub(crate) fn authenticates_network_config(&self, config: &OciNetworkConfig) -> bool {
        config.attachment_id == self.attachment_id
            && config.reservation_claim == self.reservation_claim
            && config.segment_id == self.segment_id.as_str()
            && matches!(
                (self.backend, config.provider_kind()),
                (
                    AttachmentBackendKind::Container,
                    OciAttachmentProviderKind::Container
                ) | (AttachmentBackendKind::Krun, OciAttachmentProviderKind::Krun)
            )
    }
}

/// Result from one backend-owned cleanup-context adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OciOrphanCleanupDisposition {
    /// Exact context is incomplete or still live. Keep every quarantine fence.
    Retain,
    /// The existing provider/IPAM/port/segment lifecycle reached terminal state.
    Converged,
}

/// Small backend capability for authenticating cleanup context and composing
/// the existing OCI lifecycle. Implementations cannot select their subject.
pub(crate) trait OciOrphanCleanupContext {
    fn converge_quarantined_orphan(
        &self,
        subject: &OciOrphanCleanupSubject,
    ) -> Result<OciOrphanCleanupDisposition>;
}

/// Compile only closed cleanup rows from the frozen evidence snapshot.
pub(in crate::backends::oci::network) fn compile_cleanup_subject(
    candidate: &OciOrphanEvidenceCandidate,
    reason: OciOrphanQuarantineReason,
) -> Option<OciOrphanCleanupSubject> {
    compile_terminal_publication_subject(candidate, reason)
        .or_else(|| compile_effectful_cleanup_subject(candidate, reason))
        .or_else(|| compile_never_effected_cleanup_subject(candidate, reason))
}

fn compile_effectful_cleanup_subject(
    candidate: &OciOrphanEvidenceCandidate,
    reason: OciOrphanQuarantineReason,
) -> Option<OciOrphanCleanupSubject> {
    if !matches!(
        reason,
        OciOrphanQuarantineReason::NetworkNamespaceMissing
            | OciOrphanQuarantineReason::DesiredPhaseNotAdoptable
            | OciOrphanQuarantineReason::AllocatorCleanupPending
            | OciOrphanQuarantineReason::ProviderAttemptTerminal
            | OciOrphanQuarantineReason::ProviderEffectIncomplete
            | OciOrphanQuarantineReason::ProviderStatusMissing
    ) {
        return None;
    }
    let desired = candidate.desired()?;
    let provider = candidate.provider()?;
    let backend = authenticate_effectful_identity(candidate, desired, provider)?;
    if !provider_operation_can_resume_effectful_cleanup(provider) {
        return None;
    }
    let association = desired.association();
    if association.reservation_claim() != provider.reservation_claim()
        || association.segment_id() != provider.segment_id()
    {
        return None;
    }
    if !has_effectful_cleanup_allocator_witnesses(candidate, provider, association)
        || !has_complete_effectful_artifacts(candidate)
    {
        return None;
    }
    Some(subject_from_provider(
        candidate,
        provider,
        Some(desired.clone()),
        backend,
        OciOrphanCleanupKind::Effectful,
    ))
}

fn compile_never_effected_cleanup_subject(
    candidate: &OciOrphanEvidenceCandidate,
    reason: OciOrphanQuarantineReason,
) -> Option<OciOrphanCleanupSubject> {
    if reason != OciOrphanQuarantineReason::AllocatorCleanupPending || candidate.desired().is_some()
    {
        return None;
    }
    let provider = candidate.provider()?;
    if provider.lifecycle() != OciIpamEvidenceLifecycle::Live
        || !matches!(
            provider.provider_operation(),
            NetavarkProviderOperation::Reserved
        )
        || !has_exact_absent_artifacts(candidate)
        || !provider_matches_candidate(candidate, provider)
    {
        return None;
    }
    let [allocator] = candidate.allocator() else {
        return None;
    };
    let observation = allocator.observation().ok()?;
    let association = observation.association()?;
    if allocator.source() != OciAllocatorEvidenceSource::ProviderAttempt
        || allocator.reservation_claim() != provider.reservation_claim()
        || observation.state()
            != nimbus_network::NetworkAttachmentReservationState::ReservationCleanupPending
        || association.reservation_claim() != provider.reservation_claim()
        || association.segment_id() != provider.segment_id()
    {
        return None;
    }
    Some(subject_from_provider(
        candidate,
        provider,
        None,
        attachment_backend_kind(provider.provider_kind()),
        OciOrphanCleanupKind::NeverEffected,
    ))
}

fn compile_terminal_publication_subject(
    candidate: &OciOrphanEvidenceCandidate,
    reason: OciOrphanQuarantineReason,
) -> Option<OciOrphanCleanupSubject> {
    let provider = candidate.provider()?;
    let (desired, backend) = match candidate.desired() {
        Some(desired) => {
            if reason != OciOrphanQuarantineReason::ProviderAttemptTerminal
                || desired.resource().phase() != nimbus_network::NetworkResourcePhase::Released
                || !matches!(
                    provider.provider_operation(),
                    NetavarkProviderOperation::Detached
                )
                || !has_exact_absent_allocator_witnesses(candidate, provider)
            {
                return None;
            }
            (
                Some(desired.clone()),
                authenticate_effectful_identity(candidate, desired, provider)?,
            )
        }
        None => {
            if reason != OciOrphanQuarantineReason::DesiredAttachmentMissing
                || !matches!(
                    provider.provider_operation(),
                    NetavarkProviderOperation::Reserved
                )
                || !has_exact_absent_provider_allocator_witness(candidate, provider)
                || !provider_matches_candidate(candidate, provider)
            {
                return None;
            }
            (None, attachment_backend_kind(provider.provider_kind()))
        }
    };
    if provider.lifecycle() != OciIpamEvidenceLifecycle::Terminal
        || !has_exact_absent_artifacts(candidate)
    {
        return None;
    }
    Some(subject_from_provider(
        candidate,
        provider,
        desired,
        backend,
        OciOrphanCleanupKind::TerminalPublication,
    ))
}

fn provider_operation_can_resume_effectful_cleanup(
    provider: &OciAttachmentProviderEvidence,
) -> bool {
    match provider.lifecycle() {
        OciIpamEvidenceLifecycle::Live => matches!(
            provider.provider_operation(),
            NetavarkProviderOperation::SetupPrepared { .. }
                | NetavarkProviderOperation::Provisioning { .. }
                | NetavarkProviderOperation::Ready { .. }
                | NetavarkProviderOperation::TeardownPrepared { .. }
                | NetavarkProviderOperation::NoEffectTeardownPrepared { .. }
                | NetavarkProviderOperation::Deleting { .. }
                | NetavarkProviderOperation::DetachedProjectionPending { .. }
                | NetavarkProviderOperation::Detached
        ),
        OciIpamEvidenceLifecycle::Terminal => matches!(
            provider.provider_operation(),
            NetavarkProviderOperation::Detached
        ),
    }
}

fn has_effectful_cleanup_allocator_witnesses(
    candidate: &OciOrphanEvidenceCandidate,
    provider: &OciAttachmentProviderEvidence,
    association: &NetworkAttachmentSegmentAssociation,
) -> bool {
    let mut desired_allocator = false;
    let mut provider_allocator = false;
    let mut shared_state = None;
    for evidence in candidate.allocator() {
        if evidence.reservation_claim() != provider.reservation_claim() {
            return false;
        }
        let Ok(observation) = evidence.observation() else {
            return false;
        };
        let exact = match observation.state() {
            NetworkAttachmentReservationState::Adopted
            | NetworkAttachmentReservationState::ProviderCleanupPending => {
                observation.association() == Some(association)
            }
            NetworkAttachmentReservationState::Absent => observation.association().is_none(),
            NetworkAttachmentReservationState::Reserved
            | NetworkAttachmentReservationState::ReservationCleanupPending => false,
        };
        if !exact || shared_state.is_some_and(|state| state != observation.state()) {
            return false;
        }
        shared_state = Some(observation.state());
        match evidence.source() {
            OciAllocatorEvidenceSource::DesiredAttachment if !desired_allocator => {
                desired_allocator = true;
            }
            OciAllocatorEvidenceSource::ProviderAttempt if !provider_allocator => {
                provider_allocator = true;
            }
            _ => return false,
        }
    }
    desired_allocator && provider_allocator
}

fn has_complete_effectful_artifacts(candidate: &OciOrphanEvidenceCandidate) -> bool {
    let mut manifest = None;
    let mut namespace = None;
    let mut status = None;
    for artifact in candidate.artifacts() {
        let slot = match artifact.kind() {
            OciArtifactKind::Manifest => &mut manifest,
            OciArtifactKind::NetworkNamespace => &mut namespace,
            OciArtifactKind::Status => &mut status,
        };
        if slot.replace(artifact.state()).is_some() {
            return false;
        }
    }
    matches!(manifest, Some(OciArtifactObservationState::Present))
        && matches!(
            namespace,
            Some(OciArtifactObservationState::Present | OciArtifactObservationState::Absent)
        )
        && matches!(
            status,
            Some(OciArtifactObservationState::Present | OciArtifactObservationState::Absent)
        )
}

fn authenticate_effectful_identity(
    candidate: &OciOrphanEvidenceCandidate,
    desired: &DurableNetworkAttachmentState,
    provider: &OciAttachmentProviderEvidence,
) -> Option<AttachmentBackendKind> {
    if !provider_matches_candidate(candidate, provider)
        || desired.tenant_id() != candidate.tenant_id()
        || desired.attachment_id().ok()? != candidate.attachment_id()
    {
        return None;
    }
    let backend = attachment_backend_kind(provider.provider_kind());
    let expected_handle = oci_attachment_provider_handle_for_identity(
        desired.resource().version().plan_id(),
        candidate.attachment_id(),
        backend,
    )
    .ok()?;
    if desired.selected_provider_id() != expected_handle.provider_id()
        || desired.resource().provider_handle() != Some(&expected_handle)
    {
        return None;
    }
    Some(backend)
}

fn provider_matches_candidate(
    candidate: &OciOrphanEvidenceCandidate,
    provider: &OciAttachmentProviderEvidence,
) -> bool {
    provider.tenant_id() == candidate.tenant_id()
        && provider.attachment_id() == candidate.attachment_id()
}

fn has_exact_absent_allocator_witnesses(
    candidate: &OciOrphanEvidenceCandidate,
    provider: &OciAttachmentProviderEvidence,
) -> bool {
    let mut desired_allocator = false;
    let mut provider_allocator = false;
    for evidence in candidate.allocator() {
        if evidence.reservation_claim() != provider.reservation_claim()
            || !matches!(
                evidence.observation(),
                Ok(observation)
                    if observation.state()
                        == nimbus_network::NetworkAttachmentReservationState::Absent
            )
        {
            return false;
        }
        match evidence.source() {
            OciAllocatorEvidenceSource::DesiredAttachment if !desired_allocator => {
                desired_allocator = true;
            }
            OciAllocatorEvidenceSource::ProviderAttempt if !provider_allocator => {
                provider_allocator = true;
            }
            _ => return false,
        }
    }
    desired_allocator && provider_allocator
}

fn has_exact_absent_provider_allocator_witness(
    candidate: &OciOrphanEvidenceCandidate,
    provider: &OciAttachmentProviderEvidence,
) -> bool {
    let [evidence] = candidate.allocator() else {
        return false;
    };
    evidence.source() == OciAllocatorEvidenceSource::ProviderAttempt
        && evidence.reservation_claim() == provider.reservation_claim()
        && matches!(
            evidence.observation(),
            Ok(observation)
                if observation.state() == NetworkAttachmentReservationState::Absent
                    && observation.association().is_none()
        )
}

fn has_exact_absent_artifacts(candidate: &OciOrphanEvidenceCandidate) -> bool {
    let mut manifest_present = false;
    let mut observed_namespace_absent = false;
    let mut observed_status_absent = false;
    for artifact in candidate.artifacts() {
        match (artifact.kind(), artifact.state()) {
            (OciArtifactKind::Manifest, OciArtifactObservationState::Present) => {
                manifest_present = true;
            }
            (OciArtifactKind::NetworkNamespace, OciArtifactObservationState::Absent) => {
                observed_namespace_absent = true;
            }
            (OciArtifactKind::Status, OciArtifactObservationState::Absent) => {
                observed_status_absent = true;
            }
            _ => {}
        }
    }
    manifest_present && observed_namespace_absent && observed_status_absent
}

fn subject_from_provider(
    candidate: &OciOrphanEvidenceCandidate,
    provider: &OciAttachmentProviderEvidence,
    desired: Option<DurableNetworkAttachmentState>,
    backend: AttachmentBackendKind,
    kind: OciOrphanCleanupKind,
) -> OciOrphanCleanupSubject {
    OciOrphanCleanupSubject {
        tenant_id: candidate.tenant_id().clone(),
        sandbox_id: provider.sandbox_id().clone(),
        attachment_id: candidate.attachment_id().clone(),
        segment_id: provider.segment_id().clone(),
        reservation_claim: provider.reservation_claim().clone(),
        desired,
        backend,
        kind,
    }
}

const fn attachment_backend_kind(
    provider_kind: OciAttachmentProviderKind,
) -> AttachmentBackendKind {
    match provider_kind {
        OciAttachmentProviderKind::Container => AttachmentBackendKind::Container,
        OciAttachmentProviderKind::Krun => AttachmentBackendKind::Krun,
    }
}
