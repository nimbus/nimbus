//! Pure disposition selection over an immutable OCI orphan-evidence snapshot.
//!
//! NNC5.2d owns applying quarantine and startup wiring. This module has no
//! filesystem, provider, mutable-authority, cleanup, release, or reuse
//! capability.

use nimbus_network::{
    NetworkAttachmentReservationState, NetworkResourcePhase, NetworkResourceVersion,
};

use super::{
    OciAllocatorEvidenceSource, OciArtifactKind, OciArtifactObservation,
    OciArtifactObservationState, OciEvidenceUnknown, OciOrphanEvidenceCandidate,
    OciOrphanEvidenceReport, OciProviderRealmObservation, OciUnmatchedProviderEvidence,
};
use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_provider_id,
};
use crate::backends::oci::network::attachment_lifecycle::{
    AttachmentBackendKind, oci_attachment_plan, oci_attachment_provider_handle,
};
use crate::backends::oci::network::default_network_attachment_id;
use crate::backends::oci::network::dto::NetavarkProviderOperation;
use crate::backends::oci::network::ipam::OciIpamEvidenceLifecycle;
use crate::backends::oci::network::provider_locator::OciAttachmentProviderKind;

/// A safe read-only disposition for one startup evidence subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::backends::oci::network) enum OciOrphanDisposition {
    /// Every authoritative and required observed field describes one current
    /// generation. Adoption itself performs no mutation.
    Adopt,
    /// The subject must remain fenced for a named, deterministic reason.
    Quarantine(OciOrphanQuarantineReason),
}

/// Closed fail-safe reasons emitted by the pure classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::backends::oci::network) enum OciOrphanQuarantineReason {
    DesiredAttachmentMissing,
    ProviderAttemptMissing,
    ProviderAttemptTerminal,
    ProviderBackendMismatch,
    StaleGenerationEvidence,
    DesiredPhaseNotAdoptable,
    DesiredProviderHandleMissing,
    DesiredProviderHandleMismatch,
    AllocatorEvidenceIncomplete,
    AllocatorHoldMissing,
    AllocatorReservationUnadopted,
    AllocatorCleanupPending,
    ProviderEffectIncomplete,
    ArtifactEvidenceIncomplete,
    NetworkNamespaceMissing,
    ProviderStatusMissing,
    UnknownInspection,
    ProviderRealmMismatch,
    UnmatchedArtifact,
}

/// One immutable evidence subject paired with its pure disposition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::backends::oci::network) struct OciEvidenceClassification<'a, Evidence> {
    evidence: &'a Evidence,
    disposition: OciOrphanDisposition,
}

impl<'a, Evidence> OciEvidenceClassification<'a, Evidence> {
    pub(in crate::backends::oci::network) fn evidence(&self) -> &'a Evidence {
        self.evidence
    }

    pub(in crate::backends::oci::network) fn disposition(&self) -> OciOrphanDisposition {
        self.disposition
    }
}

/// Classifications retain exact references and deterministic report ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::backends::oci::network) struct OciOrphanClassificationReport<'a> {
    candidate_classifications: Vec<OciEvidenceClassification<'a, OciOrphanEvidenceCandidate>>,
    unmatched_provider_classifications:
        Vec<OciEvidenceClassification<'a, OciUnmatchedProviderEvidence>>,
    unmatched_artifact_classifications: Vec<OciEvidenceClassification<'a, OciArtifactObservation>>,
    artifact_scan_unknown_classifications: Vec<OciEvidenceClassification<'a, OciEvidenceUnknown>>,
}

impl<'a> OciOrphanClassificationReport<'a> {
    pub(in crate::backends::oci::network) fn candidate_classifications(
        &self,
    ) -> &[OciEvidenceClassification<'a, OciOrphanEvidenceCandidate>] {
        &self.candidate_classifications
    }

    pub(in crate::backends::oci::network) fn unmatched_provider_classifications(
        &self,
    ) -> &[OciEvidenceClassification<'a, OciUnmatchedProviderEvidence>] {
        &self.unmatched_provider_classifications
    }

    pub(in crate::backends::oci::network) fn unmatched_artifact_classifications(
        &self,
    ) -> &[OciEvidenceClassification<'a, OciArtifactObservation>] {
        &self.unmatched_artifact_classifications
    }

    pub(in crate::backends::oci::network) fn artifact_scan_unknown_classifications(
        &self,
    ) -> &[OciEvidenceClassification<'a, OciEvidenceUnknown>] {
        &self.artifact_scan_unknown_classifications
    }
}

/// Classify every immutable evidence subject without applying the result.
pub(in crate::backends::oci::network) fn classify_oci_orphan_evidence<'a>(
    report: &'a OciOrphanEvidenceReport,
) -> OciOrphanClassificationReport<'a> {
    OciOrphanClassificationReport {
        candidate_classifications: report
            .candidates()
            .iter()
            .map(|evidence| OciEvidenceClassification {
                evidence,
                disposition: classify_candidate(evidence),
            })
            .collect(),
        unmatched_provider_classifications: report
            .unmatched_provider_evidence()
            .iter()
            .map(|evidence| OciEvidenceClassification {
                disposition: match evidence.realm() {
                    OciProviderRealmObservation::DifferentRealm => {
                        quarantine(OciOrphanQuarantineReason::ProviderRealmMismatch)
                    }
                    OciProviderRealmObservation::Unknown(_) => {
                        quarantine(OciOrphanQuarantineReason::UnknownInspection)
                    }
                },
                evidence,
            })
            .collect(),
        unmatched_artifact_classifications: report
            .unmatched_artifacts()
            .iter()
            .map(|evidence| OciEvidenceClassification {
                disposition: match evidence.state() {
                    OciArtifactObservationState::Unknown(_) => {
                        quarantine(OciOrphanQuarantineReason::UnknownInspection)
                    }
                    OciArtifactObservationState::Present | OciArtifactObservationState::Absent => {
                        quarantine(OciOrphanQuarantineReason::UnmatchedArtifact)
                    }
                },
                evidence,
            })
            .collect(),
        artifact_scan_unknown_classifications: report
            .artifact_scan_unknowns()
            .iter()
            .map(|evidence| OciEvidenceClassification {
                evidence,
                disposition: quarantine(OciOrphanQuarantineReason::UnknownInspection),
            })
            .collect(),
    }
}

fn classify_candidate(candidate: &OciOrphanEvidenceCandidate) -> OciOrphanDisposition {
    let Some(desired) = candidate.desired() else {
        return quarantine(OciOrphanQuarantineReason::DesiredAttachmentMissing);
    };
    let Some(provider) = candidate.provider() else {
        return quarantine(OciOrphanQuarantineReason::ProviderAttemptMissing);
    };

    let desired_attachment_id = match desired.attachment_id() {
        Ok(attachment_id) => attachment_id,
        Err(_) => return quarantine(OciOrphanQuarantineReason::StaleGenerationEvidence),
    };
    if desired.tenant_id() != candidate.tenant_id()
        || desired_attachment_id != candidate.attachment_id()
        || provider.tenant_id() != candidate.tenant_id()
        || provider.attachment_id() != candidate.attachment_id()
    {
        return quarantine(OciOrphanQuarantineReason::StaleGenerationEvidence);
    }

    if desired.selected_provider_id() != &selected_provider_id(provider.provider_kind()) {
        return quarantine(OciOrphanQuarantineReason::ProviderBackendMismatch);
    }

    let association = desired.association();
    if association.reservation_claim() != provider.reservation_claim()
        || association.segment_id() != provider.segment_id()
    {
        return quarantine(OciOrphanQuarantineReason::StaleGenerationEvidence);
    }
    if default_network_attachment_id(provider.sandbox_id()) != *candidate.attachment_id() {
        return quarantine(OciOrphanQuarantineReason::StaleGenerationEvidence);
    }
    let backend = attachment_backend_kind(provider.provider_kind());
    let expected_plan = oci_attachment_plan(candidate.tenant_id(), provider.sandbox_id(), backend);
    let expected_version = NetworkResourceVersion::for_plan(
        &expected_plan,
        candidate.attachment_id().clone().into(),
        association.lease_epoch(),
    );
    if desired
        .resource()
        .authenticate_version(&expected_version)
        .is_err()
    {
        return quarantine(OciOrphanQuarantineReason::StaleGenerationEvidence);
    }

    if candidate
        .allocator()
        .iter()
        .any(|evidence| evidence.observation().is_err())
        || candidate
            .artifacts()
            .iter()
            .any(|artifact| matches!(artifact.state(), OciArtifactObservationState::Unknown(_)))
    {
        return quarantine(OciOrphanQuarantineReason::UnknownInspection);
    }

    if provider.lifecycle() != OciIpamEvidenceLifecycle::Live {
        return quarantine(OciOrphanQuarantineReason::ProviderAttemptTerminal);
    }

    let desired_phase = desired.resource().phase();
    if !matches!(
        desired_phase,
        NetworkResourcePhase::Provisioning
            | NetworkResourcePhase::Ready
            | NetworkResourcePhase::Publishing
            | NetworkResourcePhase::Active
    ) {
        return quarantine(OciOrphanQuarantineReason::DesiredPhaseNotAdoptable);
    }
    let expected_provider_handle =
        match oci_attachment_provider_handle(candidate.tenant_id(), provider.sandbox_id(), backend)
        {
            Ok(provider_handle) => provider_handle,
            Err(_) => {
                return quarantine(OciOrphanQuarantineReason::DesiredProviderHandleMismatch);
            }
        };
    match desired.resource().provider_handle() {
        Some(provider_handle) if provider_handle != &expected_provider_handle => {
            return quarantine(OciOrphanQuarantineReason::DesiredProviderHandleMismatch);
        }
        None if matches!(
            desired_phase,
            NetworkResourcePhase::Ready
                | NetworkResourcePhase::Publishing
                | NetworkResourcePhase::Active
        ) =>
        {
            return quarantine(OciOrphanQuarantineReason::DesiredProviderHandleMissing);
        }
        Some(_) | None => {}
    }

    let mut desired_allocator = None;
    let mut provider_allocator = None;
    for evidence in candidate.allocator() {
        let slot = match evidence.source() {
            OciAllocatorEvidenceSource::DesiredAttachment => &mut desired_allocator,
            OciAllocatorEvidenceSource::ProviderAttempt => &mut provider_allocator,
        };
        if slot.replace(evidence).is_some() {
            return quarantine(OciOrphanQuarantineReason::AllocatorEvidenceIncomplete);
        }
    }
    let (Some(desired_allocator), Some(provider_allocator)) =
        (desired_allocator, provider_allocator)
    else {
        return quarantine(OciOrphanQuarantineReason::AllocatorEvidenceIncomplete);
    };

    for evidence in [desired_allocator, provider_allocator] {
        if evidence.reservation_claim() != association.reservation_claim() {
            return quarantine(OciOrphanQuarantineReason::StaleGenerationEvidence);
        }
        let observation = match evidence.observation() {
            Ok(observation) => observation,
            Err(_) => return quarantine(OciOrphanQuarantineReason::UnknownInspection),
        };
        if let Some(observed_association) = observation.association()
            && observed_association != association
        {
            return quarantine(OciOrphanQuarantineReason::StaleGenerationEvidence);
        }
        match observation.state() {
            NetworkAttachmentReservationState::Absent => {
                return quarantine(OciOrphanQuarantineReason::AllocatorHoldMissing);
            }
            NetworkAttachmentReservationState::Reserved => {
                return quarantine(OciOrphanQuarantineReason::AllocatorReservationUnadopted);
            }
            NetworkAttachmentReservationState::ReservationCleanupPending
            | NetworkAttachmentReservationState::ProviderCleanupPending => {
                return quarantine(OciOrphanQuarantineReason::AllocatorCleanupPending);
            }
            NetworkAttachmentReservationState::Adopted => {
                if observation.association().is_none() {
                    return quarantine(OciOrphanQuarantineReason::AllocatorEvidenceIncomplete);
                }
            }
        }
    }

    let mut manifest = None;
    let mut network_namespace = None;
    let mut status = None;
    for artifact in candidate.artifacts() {
        let slot = match artifact.kind() {
            OciArtifactKind::Manifest => &mut manifest,
            OciArtifactKind::NetworkNamespace => &mut network_namespace,
            OciArtifactKind::Status => &mut status,
        };
        if slot.replace(artifact.state()).is_some() {
            return quarantine(OciOrphanQuarantineReason::ArtifactEvidenceIncomplete);
        }
    }
    let (Some(manifest), Some(network_namespace), Some(status)) =
        (manifest, network_namespace, status)
    else {
        return quarantine(OciOrphanQuarantineReason::ArtifactEvidenceIncomplete);
    };
    if matches!(manifest, OciArtifactObservationState::Unknown(_)) {
        return quarantine(OciOrphanQuarantineReason::UnknownInspection);
    }

    if !matches!(
        provider.provider_operation(),
        NetavarkProviderOperation::Ready { .. }
    ) {
        return quarantine(OciOrphanQuarantineReason::ProviderEffectIncomplete);
    }
    if matches!(network_namespace, OciArtifactObservationState::Absent) {
        return quarantine(OciOrphanQuarantineReason::NetworkNamespaceMissing);
    }
    if matches!(status, OciArtifactObservationState::Absent) {
        return quarantine(OciOrphanQuarantineReason::ProviderStatusMissing);
    }

    OciOrphanDisposition::Adopt
}

fn selected_provider_id(
    provider_kind: OciAttachmentProviderKind,
) -> nimbus_network::NetworkProviderId {
    host_managed_attachment_provider_id(match provider_kind {
        OciAttachmentProviderKind::Container => SandboxAttachmentRegistrationKind::Container,
        OciAttachmentProviderKind::Krun => SandboxAttachmentRegistrationKind::Krun,
    })
}

const fn attachment_backend_kind(
    provider_kind: OciAttachmentProviderKind,
) -> AttachmentBackendKind {
    match provider_kind {
        OciAttachmentProviderKind::Container => AttachmentBackendKind::Container,
        OciAttachmentProviderKind::Krun => AttachmentBackendKind::Krun,
    }
}

const fn quarantine(reason: OciOrphanQuarantineReason) -> OciOrphanDisposition {
    OciOrphanDisposition::Quarantine(reason)
}

#[cfg(test)]
mod tests;
