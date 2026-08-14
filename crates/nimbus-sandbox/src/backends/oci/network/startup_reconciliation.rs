//! Evidence-aware startup quarantine for OCI-family network authorities.
//!
//! This owner can read the complete durable/observed snapshot and apply exact,
//! fenced quarantine transitions. After a fence is durable, it can offer an
//! identity-only subject to an injected backend cleanup context. It has no
//! provider, artifact-removal, release, finalization, or capacity-reuse
//! capability of its own.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use nimbus_network::{
    DurableNetworkAttachmentState, LocalNetworkAttachmentAuthority,
    NetworkAttachmentReservationState, NetworkReservationClaim, NetworkResourcePhase,
    NetworkSegmentQuarantineOutcome, NetworkStateTransition, NetworkTransitionEvidence,
};

use super::orphan_convergence::{
    OciOrphanCleanupContext, OciOrphanCleanupDisposition, compile_cleanup_subject,
};
use super::orphan_evidence::{
    OciEvidenceUnknown, OciOrphanDisposition, OciOrphanEvidenceCandidate,
    OciOrphanQuarantineReason, classify_oci_orphan_evidence, classify_retained_desired_manifest,
    collect_oci_orphan_evidence,
};
use super::{OciIpamAuthority, OciSegmentAllocator};
use crate::error::{Result, SandboxError};

/// Classify the complete startup snapshot and durably fence every unsafe
/// subject without a cleanup context.
///
/// Exact adoption is entirely read-only. Any quarantine classification keeps
/// admission closed even when its available durable authorities were
/// successfully transitioned: later cleanup convergence, not startup, owns
/// proving absence and reopening capacity.
#[cfg(test)]
pub(crate) fn reconcile_startup_network_state(
    workload_state_root: &Path,
    attachments: &LocalNetworkAttachmentAuthority,
    ipam: &OciIpamAuthority,
    allocator: &OciSegmentAllocator,
) -> Result<()> {
    reconcile_startup_network_state_with_retained_desired_manifests(
        workload_state_root,
        attachments,
        ipam,
        allocator,
        &BTreeSet::new(),
    )
}

/// Reconcile network authority while retaining a strict set of claim-only
/// desired manifests authenticated by the container manifest owner.
///
/// Retention is read-only and applies only to an otherwise-unmatched manifest
/// path. Any attachment, provider, allocator, namespace, status, or unknown
/// evidence keeps the normal classifier and quarantine behavior.
#[cfg(test)]
pub(crate) fn reconcile_startup_network_state_with_retained_desired_manifests(
    workload_state_root: &Path,
    attachments: &LocalNetworkAttachmentAuthority,
    ipam: &OciIpamAuthority,
    allocator: &OciSegmentAllocator,
    retained_desired_manifests: &BTreeSet<PathBuf>,
) -> Result<()> {
    reconcile_startup_network_state_with_optional_cleanup(
        workload_state_root,
        attachments,
        ipam,
        allocator,
        retained_desired_manifests,
        None,
    )
}

/// Apply exact startup quarantine, then offer only proven effectful orphans to
/// a backend-owned cleanup-context adapter.
pub(crate) fn reconcile_startup_network_state_with_cleanup(
    workload_state_root: &Path,
    attachments: &LocalNetworkAttachmentAuthority,
    ipam: &OciIpamAuthority,
    allocator: &OciSegmentAllocator,
    retained_desired_manifests: &BTreeSet<PathBuf>,
    cleanup: &dyn OciOrphanCleanupContext,
) -> Result<()> {
    reconcile_startup_network_state_with_optional_cleanup(
        workload_state_root,
        attachments,
        ipam,
        allocator,
        retained_desired_manifests,
        Some(cleanup),
    )
}

fn reconcile_startup_network_state_with_optional_cleanup(
    workload_state_root: &Path,
    attachments: &LocalNetworkAttachmentAuthority,
    ipam: &OciIpamAuthority,
    allocator: &OciSegmentAllocator,
    retained_desired_manifests: &BTreeSet<PathBuf>,
    cleanup: Option<&dyn OciOrphanCleanupContext>,
) -> Result<()> {
    let report = collect_oci_orphan_evidence(workload_state_root, attachments, ipam, allocator)?;
    let classifications = classify_oci_orphan_evidence(&report);
    let mut fences = Vec::new();

    for classification in classifications.candidate_classifications() {
        let reason = match classification.disposition() {
            OciOrphanDisposition::Adopt => continue,
            OciOrphanDisposition::Quarantine(reason) => reason,
        };
        let candidate = classification.evidence();
        let application = quarantine_candidate(attachments, allocator, candidate);
        let fence = match application {
            Ok(()) => {
                if let (Some(cleanup), Some(subject)) =
                    (cleanup, compile_cleanup_subject(candidate, reason))
                {
                    match cleanup.converge_quarantined_orphan(&subject) {
                        Ok(OciOrphanCleanupDisposition::Converged) => continue,
                        Ok(OciOrphanCleanupDisposition::Retain) => {}
                        Err(error) => {
                            fences.push(format!(
                                "tenant {} attachment {}: {}; exact orphan cleanup failed: {error}",
                                candidate.tenant_id(),
                                candidate.attachment_id(),
                                reason.as_str()
                            ));
                            continue;
                        }
                    }
                }
                format!(
                    "tenant {} attachment {}: {}",
                    candidate.tenant_id(),
                    candidate.attachment_id(),
                    reason.as_str()
                )
            }
            Err(error) => format!(
                "tenant {} attachment {}: {}; exact quarantine application failed: {error}",
                candidate.tenant_id(),
                candidate.attachment_id(),
                reason.as_str()
            ),
        };
        fences.push(fence);
    }

    for classification in classifications.unmatched_provider_classifications() {
        let OciOrphanDisposition::Quarantine(reason) = classification.disposition() else {
            continue;
        };
        let evidence = classification.evidence().evidence();
        fences.push(format!(
            "tenant {} attachment {}: {}",
            evidence.tenant_id(),
            evidence.attachment_id(),
            reason.as_str()
        ));
    }

    for classification in classifications.unmatched_artifact_classifications() {
        if matches!(
            classify_retained_desired_manifest(
                classification.evidence(),
                retained_desired_manifests,
            ),
            Some(OciOrphanDisposition::Adopt)
        ) {
            continue;
        }
        let OciOrphanDisposition::Quarantine(reason) = classification.disposition() else {
            continue;
        };
        let artifact = classification.evidence();
        fences.push(format!(
            "{} at {}: {}",
            artifact.kind().as_str(),
            artifact.path().display(),
            reason.as_str()
        ));
    }

    for classification in classifications.artifact_scan_unknown_classifications() {
        let OciOrphanDisposition::Quarantine(reason) = classification.disposition() else {
            continue;
        };
        fences.push(format_unknown(classification.evidence(), reason));
    }

    if fences.is_empty() {
        Ok(())
    } else {
        Err(SandboxError::OperationFailed {
            message: format!(
                "startup network reconciliation quarantined evidence under {}: {}",
                workload_state_root.display(),
                fences.join("; ")
            ),
        })
    }
}

/// Apply the two independent exact authority transitions in crash-safe order:
/// desired generation first, allocator claim second.
fn quarantine_candidate(
    attachments: &LocalNetworkAttachmentAuthority,
    allocator: &OciSegmentAllocator,
    candidate: &OciOrphanEvidenceCandidate,
) -> Result<()> {
    let exact_claim = exact_adopted_claim(candidate);
    if let Some(desired) = candidate.desired() {
        quarantine_desired_generation(attachments, candidate, desired)?;
    }
    let Some(exact_claim) = exact_claim else {
        return Ok(());
    };
    match allocator.quarantine(
        candidate.tenant_id(),
        candidate.attachment_id(),
        Some(exact_claim),
    )? {
        NetworkSegmentQuarantineOutcome::CleanupPending => Ok(()),
        NetworkSegmentQuarantineOutcome::AlreadyReleased => Err(SandboxError::OperationFailed {
            message: "allocator authority disappeared after exact adopted evidence was read"
                .to_owned(),
        }),
    }
}

fn quarantine_desired_generation(
    attachments: &LocalNetworkAttachmentAuthority,
    candidate: &OciOrphanEvidenceCandidate,
    desired: &DurableNetworkAttachmentState,
) -> Result<()> {
    let Ok(desired_attachment_id) = desired.attachment_id() else {
        return Ok(());
    };
    if desired.tenant_id() != candidate.tenant_id()
        || desired_attachment_id != candidate.attachment_id()
    {
        return Ok(());
    }
    if !matches!(
        desired.resource().phase(),
        NetworkResourcePhase::Provisioning
            | NetworkResourcePhase::Ready
            | NetworkResourcePhase::Publishing
            | NetworkResourcePhase::Active
            | NetworkResourcePhase::Withdrawing
            | NetworkResourcePhase::Draining
            | NetworkResourcePhase::Deleting
            | NetworkResourcePhase::CleanupPending
    ) {
        return Ok(());
    }
    attachments
        .apply_transition(
            candidate.tenant_id(),
            &NetworkStateTransition::new(
                desired.resource().version().clone(),
                NetworkResourcePhase::CleanupPending,
                NetworkTransitionEvidence::AmbiguousEffect,
            ),
        )
        .map(|_| ())
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "durable attachment authority rejected exact startup quarantine: {error}"
            ),
        })
}

/// Return a claim only when every injected allocator observation proves the
/// same adopted association and every durable source agrees with it.
fn exact_adopted_claim(candidate: &OciOrphanEvidenceCandidate) -> Option<&NetworkReservationClaim> {
    let first = candidate.allocator().first()?;
    let claim = first.reservation_claim();
    for evidence in candidate.allocator() {
        if evidence.reservation_claim() != claim {
            return None;
        }
        let observation = evidence.observation().ok()?;
        if observation.state() != NetworkAttachmentReservationState::Adopted {
            return None;
        }
        let association = observation.association()?;
        if association.reservation_claim() != claim {
            return None;
        }
        if candidate
            .desired()
            .is_some_and(|desired| desired.association() != association)
        {
            return None;
        }
        if candidate.provider().is_some_and(|provider| {
            provider.reservation_claim() != claim
                || provider.segment_id() != association.segment_id()
        }) {
            return None;
        }
    }
    Some(claim)
}

fn format_unknown(unknown: &OciEvidenceUnknown, reason: OciOrphanQuarantineReason) -> String {
    let path = unknown
        .path()
        .map(|path| format!(" at {}", path.display()))
        .unwrap_or_default();
    format!(
        "{}{} ({}: {}): {}",
        unknown.operation(),
        path,
        unknown.error_kind(),
        unknown.message(),
        reason.as_str()
    )
}

#[cfg(test)]
mod tests;
