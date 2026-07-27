//! Attempt-scoped attachment reservation and adoption.

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAttachmentId, NetworkReservationClaim, NetworkSegmentId, NetworkSegmentReleaseOutcome,
};

use crate::error::{Result, SandboxError};

use super::{OciSegmentRealization, SegmentAttachmentState, SingleNodeSegmentAllocator};

impl SingleNodeSegmentAllocator {
    pub(super) fn reserve_attachment_for_coordinator_inner(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<()> {
        let supernet = self.installed()?.clone();
        self.with_state(|state| {
            self.assign_block(&supernet, state, tenant)?;
            let entry = state
                .tenants
                .get_mut(tenant.as_str())
                .expect("assign_block inserts the tenant entry");
            if entry.allocation_cleanup_pending {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "network segment allocation for tenant {} is cleanup-pending; refusing reservation until provider deletion is confirmed",
                        tenant.as_str()
                    ),
                });
            }
            match entry.attachments.get(attachment_id.as_str()) {
                Some(SegmentAttachmentState::UnplacedReserved {
                    reservation_claim: existing,
                }) if existing == reservation_claim => return Ok(()),
                Some(SegmentAttachmentState::UnplacedReserved { .. }) => {
                    return Err(reservation_claim_conflict(attachment_id));
                }
                Some(SegmentAttachmentState::Reserved {
                    reservation_claim: existing,
                    ..
                }) if existing == reservation_claim => return Ok(()),
                Some(SegmentAttachmentState::Reserved { .. }) => {
                    return Err(reservation_claim_conflict(attachment_id));
                }
                Some(SegmentAttachmentState::ReservationCleanupPending { .. }) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} has exact reservation cleanup pending; refusing reservation until IPAM deletion is confirmed",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::Held { .. }) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} is already adopted; refusing launch reservation",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::CleanupPending { .. }) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} is cleanup-pending; refusing reservation until provider deletion is confirmed",
                            attachment_id.as_str()
                        ),
                    });
                }
                None => {}
            }
            entry.attachments.insert(
                attachment_id.as_str().to_owned(),
                SegmentAttachmentState::UnplacedReserved {
                    reservation_claim: reservation_claim.clone(),
                },
            );
            Ok(())
        })
    }

    pub(super) fn bind_reserved_attachment_to_segment_inner(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        segment_id: &NetworkSegmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<OciSegmentRealization> {
        let supernet = self.installed()?.clone();
        let block = self.with_state(|state| {
            self.ensure_supernet_matches(&supernet, state)?;
            let entry = state.tenants.get_mut(tenant.as_str()).ok_or_else(|| {
                SandboxError::OperationFailed {
                    message: format!(
                        "network attachment {} has no durable segment reservation",
                        attachment_id.as_str()
                    ),
                }
            })?;
            let block = entry
                .blocks
                .iter()
                .find(|block| &block.segment_id == segment_id)
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "network attachment {} cannot bind unknown tenant segment {segment_id}",
                        attachment_id.as_str()
                    ),
                })?;
            match entry.attachments.get(attachment_id.as_str()) {
                Some(SegmentAttachmentState::UnplacedReserved {
                    reservation_claim: existing,
                }) if existing == reservation_claim => {
                    entry.attachments.insert(
                        attachment_id.as_str().to_owned(),
                        SegmentAttachmentState::Reserved {
                            reservation_claim: reservation_claim.clone(),
                            segment_id: segment_id.clone(),
                        },
                    );
                }
                Some(SegmentAttachmentState::UnplacedReserved { .. }) => {
                    return Err(reservation_claim_conflict(attachment_id));
                }
                Some(SegmentAttachmentState::Reserved {
                    reservation_claim: existing,
                    segment_id: existing_segment,
                }) if existing == reservation_claim && existing_segment == segment_id => {}
                Some(SegmentAttachmentState::Reserved {
                    reservation_claim: existing,
                    segment_id: existing_segment,
                }) if existing == reservation_claim => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} is already bound to segment {existing_segment}; refusing remap to {segment_id}",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::ReservationCleanupPending { .. }) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} has exact reservation cleanup pending and cannot be placed",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::Held { .. })
                | Some(SegmentAttachmentState::CleanupPending { .. }) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} is already adopted and cannot be remapped",
                            attachment_id.as_str()
                        ),
                    });
                }
                None => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} has no durable segment reservation",
                            attachment_id.as_str()
                        ),
                    });
                }
                Some(SegmentAttachmentState::Reserved { .. }) => {
                    return Err(reservation_claim_conflict(attachment_id));
                }
            }
            Ok(block)
        })?;
        self.segment_at(&supernet, tenant, &block)
    }

    pub(super) fn adopt_reserved_attachment_inner(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<OciSegmentRealization> {
        let supernet = self.installed()?.clone();
        let block = self.with_state(|state| {
            self.ensure_supernet_matches(&supernet, state)?;
            let entry = state.tenants.get_mut(tenant.as_str()).ok_or_else(|| {
                SandboxError::OperationFailed {
                    message: format!(
                        "network attachment {} has no durable segment reservation",
                        attachment_id.as_str()
                    ),
                }
            })?;
            match entry.attachments.get(attachment_id.as_str()) {
                Some(SegmentAttachmentState::UnplacedReserved { .. }) => {
                    Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} is reserved but has no exact selected segment",
                            attachment_id.as_str()
                        ),
                    })
                }
                Some(SegmentAttachmentState::Reserved {
                    reservation_claim: existing,
                    segment_id,
                }) if existing == reservation_claim => {
                    let segment_id = segment_id.clone();
                    entry.attachments.insert(
                        attachment_id.as_str().to_owned(),
                        SegmentAttachmentState::Held {
                            adoption_receipt: Some(reservation_claim.clone()),
                            segment_id: segment_id.clone(),
                        },
                    );
                    entry
                        .blocks
                        .iter()
                        .find(|block| block.segment_id == segment_id)
                        .cloned()
                        .ok_or_else(|| SandboxError::OperationFailed {
                            message: format!(
                                "network attachment {} references missing selected segment {segment_id}",
                                attachment_id.as_str()
                            ),
                        })
                }
                Some(SegmentAttachmentState::Reserved { .. }) => {
                    Err(reservation_claim_conflict(attachment_id))
                }
                Some(SegmentAttachmentState::ReservationCleanupPending { .. }) => {
                    Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} has exact reservation cleanup pending and cannot be adopted",
                            attachment_id.as_str()
                        ),
                    })
                }
                Some(SegmentAttachmentState::Held {
                    adoption_receipt: Some(existing),
                    segment_id,
                }) if existing == reservation_claim => {
                    entry
                        .blocks
                        .iter()
                        .find(|block| block.segment_id == *segment_id)
                        .cloned()
                        .ok_or_else(|| SandboxError::OperationFailed {
                            message: format!(
                                "network attachment {} references missing selected segment {segment_id}",
                                attachment_id.as_str()
                            ),
                        })
                }
                Some(SegmentAttachmentState::Held { .. }) => {
                    Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} was acquired without this launch reservation authority",
                            attachment_id.as_str()
                        ),
                    })
                }
                Some(SegmentAttachmentState::CleanupPending { .. }) => {
                    Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} is cleanup-pending and cannot be adopted",
                            attachment_id.as_str()
                        ),
                    })
                }
                None => {
                    Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} has no durable segment reservation",
                            attachment_id.as_str()
                        ),
                    })
                }
            }
        })?;
        self.segment_at(&supernet, tenant, &block)
    }

    pub(super) fn release_reserved_attachment_without_effect_inner(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkSegmentReleaseOutcome<OciSegmentRealization>> {
        let supernet = self.installed()?.clone();
        self.with_state(|state| {
            self.ensure_supernet_matches(&supernet, state)?;
            let Some(entry) = state.tenants.get_mut(tenant.as_str()) else {
                return Ok(NetworkSegmentReleaseOutcome::AlreadyReleased);
            };
            if entry.attachments.is_empty() && entry.allocation_cleanup_pending {
                return match entry.pending_reservation_cleanup_claim.as_ref() {
                    Some(existing) if existing == reservation_claim => self
                        .cleanup_for(&supernet, tenant, entry)
                        .map(NetworkSegmentReleaseOutcome::CleanupPending),
                    Some(_) => Err(reservation_claim_conflict(attachment_id)),
                    None => Ok(NetworkSegmentReleaseOutcome::AlreadyReleased),
                };
            }
            let Some(attachment_state) =
                entry.attachments.get(attachment_id.as_str()).cloned()
            else {
                return Ok(NetworkSegmentReleaseOutcome::AlreadyReleased);
            };
            match attachment_state {
                SegmentAttachmentState::UnplacedReserved {
                    reservation_claim: existing,
                } if existing == *reservation_claim => {
                    entry.attachments.insert(
                        attachment_id.as_str().to_owned(),
                        SegmentAttachmentState::ReservationCleanupPending {
                            reservation_claim: existing,
                            segment_id: None,
                        },
                    );
                    Ok(NetworkSegmentReleaseOutcome::AttachmentCleanupPending)
                }
                SegmentAttachmentState::Reserved {
                    reservation_claim: existing,
                    segment_id,
                } if existing == *reservation_claim => {
                    entry.attachments.insert(
                        attachment_id.as_str().to_owned(),
                        SegmentAttachmentState::ReservationCleanupPending {
                            reservation_claim: existing,
                            segment_id: Some(segment_id),
                        },
                    );
                    Ok(NetworkSegmentReleaseOutcome::AttachmentCleanupPending)
                }
                SegmentAttachmentState::UnplacedReserved { .. }
                | SegmentAttachmentState::Reserved { .. } => {
                    Err(reservation_claim_conflict(attachment_id))
                }
                SegmentAttachmentState::ReservationCleanupPending {
                    reservation_claim: existing,
                    ..
                } if existing == *reservation_claim => {
                    Ok(NetworkSegmentReleaseOutcome::AttachmentCleanupPending)
                }
                SegmentAttachmentState::ReservationCleanupPending { .. } => {
                    Err(reservation_claim_conflict(attachment_id))
                }
                SegmentAttachmentState::Held { .. }
                | SegmentAttachmentState::CleanupPending { .. } => {
                    Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} is adopted and cannot use never-realized compensation",
                            attachment_id.as_str()
                        ),
                    })
                }
            }
        })
    }

    pub(super) fn finalize_reserved_attachment_without_effect_inner(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkSegmentReleaseOutcome<OciSegmentRealization>> {
        let supernet = self.installed()?.clone();
        self.with_state(|state| {
            self.ensure_supernet_matches(&supernet, state)?;
            let Some(entry) = state.tenants.get_mut(tenant.as_str()) else {
                return Ok(NetworkSegmentReleaseOutcome::AlreadyReleased);
            };
            if entry.attachments.is_empty() && entry.allocation_cleanup_pending {
                return match entry.pending_reservation_cleanup_claim.as_ref() {
                    Some(existing) if existing == reservation_claim => self
                        .cleanup_for(&supernet, tenant, entry)
                        .map(NetworkSegmentReleaseOutcome::CleanupPending),
                    Some(_) => Err(reservation_claim_conflict(attachment_id)),
                    None => Ok(NetworkSegmentReleaseOutcome::AlreadyReleased),
                };
            }
            let Some(attachment_state) =
                entry.attachments.get(attachment_id.as_str()).cloned()
            else {
                return Ok(NetworkSegmentReleaseOutcome::AlreadyReleased);
            };
            match attachment_state {
                SegmentAttachmentState::ReservationCleanupPending {
                    reservation_claim: existing,
                    ..
                } if existing == *reservation_claim => {}
                SegmentAttachmentState::ReservationCleanupPending { .. }
                | SegmentAttachmentState::UnplacedReserved { .. }
                | SegmentAttachmentState::Reserved { .. } => {
                    return Err(reservation_claim_conflict(attachment_id));
                }
                SegmentAttachmentState::Held { .. }
                | SegmentAttachmentState::CleanupPending { .. } => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "network attachment {} is adopted and cannot finalize never-realized compensation",
                            attachment_id.as_str()
                        ),
                    });
                }
            }
            entry.attachments.remove(attachment_id.as_str());
            if !entry.attachments.is_empty() {
                entry.allocation_cleanup_pending = entry
                    .attachments
                    .values()
                    .all(SegmentAttachmentState::is_cleanup_pending);
                return Ok(NetworkSegmentReleaseOutcome::StillLive);
            }
            entry.allocation_cleanup_pending = true;
            entry.pending_reservation_cleanup_claim = Some(reservation_claim.clone());
            self.cleanup_for(&supernet, tenant, entry)
                .map(NetworkSegmentReleaseOutcome::CleanupPending)
        })
    }
}

fn reservation_claim_conflict(attachment_id: &NetworkAttachmentId) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "network attachment {} belongs to a different launch reservation coordinator",
            attachment_id.as_str()
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use nimbus_network::{
        NetworkProviderHandle, NetworkProviderId, NetworkSegmentAllocator, NetworkSegmentCleanup,
        NetworkSegmentFinalizeOutcome, NetworkSegmentGrowth, NetworkSegmentId,
        NetworkSegmentQuarantineOutcome, NetworkSegmentReleaseOutcome,
    };
    use tempfile::tempdir;

    use super::*;

    fn tenant() -> TenantId {
        TenantId::new("tenant-claimed-segment").expect("tenant fixture should parse")
    }

    fn attachment(workload: &str) -> NetworkAttachmentId {
        NetworkAttachmentId::for_workload_attachment(
            workload,
            super::super::super::DEFAULT_ATTACHMENT_NAME,
        )
    }

    fn claim(attempt: &str) -> NetworkReservationClaim {
        let provider = NetworkProviderId::for_registration_key(
            "nimbus-sandbox.network-launch-coordinator.test",
        );
        NetworkReservationClaim::new(
            NetworkProviderHandle::new(provider, format!("attempt:{attempt}"))
                .expect("claim fixture should validate"),
        )
    }

    fn reserve_primary(
        allocator: &SingleNodeSegmentAllocator,
        tenant: &TenantId,
        attachment: &NetworkAttachmentId,
        claim: &NetworkReservationClaim,
    ) -> OciSegmentRealization {
        allocator
            .reserve_attachment_for_coordinator(tenant, attachment, claim)
            .expect("attachment reservation should persist");
        let segment = allocator
            .segments_for(tenant)
            .expect("tenant segments should inspect")
            .into_iter()
            .next()
            .expect("tenant reservation should own a primary segment");
        allocator
            .bind_reserved_attachment_to_segment(tenant, attachment, segment.segment_id(), claim)
            .expect("test placement should bind the primary segment")
    }

    #[test]
    fn unadopted_claim_rejects_foreign_and_generic_lifecycle_without_mutation() {
        let root = tempdir().expect("state root");
        let allocator = SingleNodeSegmentAllocator::single_node_default(root.path());
        let tenant = tenant();
        let attachment = attachment("claimed-workload");
        let owner = claim("owner");
        let foreign = claim("foreign");
        let original = reserve_primary(&allocator, &tenant, &attachment, &owner);

        for error in [
            allocator
                .reserve_attachment_for_coordinator(&tenant, &attachment, &foreign)
                .expect_err("foreign reserve must fail"),
            allocator
                .adopt_reserved_attachment(&tenant, &attachment, &foreign)
                .expect_err("foreign adoption must fail"),
            allocator
                .release_reserved_attachment_without_effect(&tenant, &attachment, &foreign)
                .expect_err("foreign compensation must fail"),
            allocator
                .acquire(&tenant, &attachment)
                .expect_err("direct acquire must not bypass a claim"),
            allocator
                .quarantine(&tenant, &attachment, None)
                .expect_err("generic quarantine must not bypass a claim"),
            allocator
                .release(&tenant, &attachment, None)
                .expect_err("generic release must not bypass a claim"),
        ] {
            assert!(
                error.to_string().contains("reservation")
                    || error.to_string().contains("different launch"),
                "claim rejection should name the durable fence: {error}"
            );
        }

        assert!(
            allocator
                .reconcile_orphans(&BTreeSet::new())
                .expect("orphan scan should succeed")
                .is_empty(),
            "filesystem absence must not quarantine an unadopted claim"
        );
        let retained = allocator
            .inspect_segments(&tenant)
            .expect("retained segment should inspect")
            .expect("foreign and generic operations must not remove the segment");
        assert_eq!(retained[0].segment_id(), original.segment_id());

        assert_eq!(
            allocator
                .release_reserved_attachment_without_effect(&tenant, &attachment, &owner)
                .expect("exact owner should fence IPAM cleanup"),
            NetworkSegmentReleaseOutcome::AttachmentCleanupPending
        );
        let cleanup = match allocator
            .finalize_reserved_attachment_without_effect(&tenant, &attachment, &owner)
            .expect("exact owner should confirm IPAM cleanup")
        {
            NetworkSegmentReleaseOutcome::CleanupPending(cleanup) => cleanup,
            outcome => panic!("last exact finalization should return cleanup, got {outcome:?}"),
        };
        assert_eq!(
            allocator
                .finalize_release(&cleanup)
                .expect("identity-fenced cleanup should finalize"),
            NetworkSegmentFinalizeOutcome::Released
        );
        assert!(
            allocator
                .inspect_segments(&tenant)
                .expect("released tenant should inspect")
                .is_none()
        );
    }

    #[test]
    fn adoption_is_exact_and_idempotent_across_authority_reopen() {
        let root = tempdir().expect("state root");
        let tenant = tenant();
        let attachment = attachment("adopted-workload");
        let owner = claim("owner");
        let foreign = claim("foreign");
        reserve_primary(
            &SingleNodeSegmentAllocator::single_node_default(root.path()),
            &tenant,
            &attachment,
            &owner,
        );

        let reopened = SingleNodeSegmentAllocator::single_node_default(root.path());
        let adopted = reopened
            .adopt_reserved_attachment(&tenant, &attachment, &owner)
            .expect("exact owner should adopt the control-plane hold");
        let replayed = SingleNodeSegmentAllocator::single_node_default(root.path())
            .adopt_reserved_attachment(&tenant, &attachment, &owner)
            .expect("same-claim acknowledgement-loss replay should succeed");
        assert_eq!(replayed.segment_id(), adopted.segment_id());
        assert!(
            reopened
                .adopt_reserved_attachment(&tenant, &attachment, &foreign)
                .is_err(),
            "foreign adoption must remain fenced after reopen"
        );
        assert!(
            reopened
                .release_reserved_attachment_without_effect(&tenant, &attachment, &owner)
                .is_err(),
            "adoption closes never-realized compensation authority"
        );

        assert_eq!(
            reopened
                .quarantine(&tenant, &attachment, Some(&owner))
                .expect("ordinary teardown should own an adopted hold"),
            NetworkSegmentQuarantineOutcome::CleanupPending
        );
        let cleanup = match reopened
            .release(&tenant, &attachment, Some(&owner))
            .expect("quarantined adopted hold should release")
        {
            NetworkSegmentReleaseOutcome::CleanupPending(cleanup) => cleanup,
            outcome => panic!("last adopted release should return cleanup, got {outcome:?}"),
        };
        assert_eq!(
            reopened
                .finalize_release(&cleanup)
                .expect("cleanup should finalize"),
            NetworkSegmentFinalizeOutcome::Released
        );
    }

    #[test]
    fn stale_adoption_receipt_cannot_quarantine_or_release_replacement_attachment() {
        let root = tempdir().expect("state root");
        let allocator = SingleNodeSegmentAllocator::single_node_default(root.path());
        let tenant = tenant();
        let attachment = attachment("replacement-workload");
        let original = claim("original-generation");
        let replacement = claim("replacement-generation");

        reserve_primary(&allocator, &tenant, &attachment, &original);
        allocator
            .adopt_reserved_attachment(&tenant, &attachment, &original)
            .expect("original generation should adopt");
        allocator
            .quarantine(&tenant, &attachment, Some(&original))
            .expect("original generation should quarantine");
        let original_cleanup = match allocator
            .release(&tenant, &attachment, Some(&original))
            .expect("original generation should release")
        {
            NetworkSegmentReleaseOutcome::CleanupPending(cleanup) => cleanup,
            outcome => panic!("original last hold should require cleanup, got {outcome:?}"),
        };
        allocator
            .finalize_release(&original_cleanup)
            .expect("original allocation should finalize");

        reserve_primary(&allocator, &tenant, &attachment, &replacement);
        allocator
            .adopt_reserved_attachment(&tenant, &attachment, &replacement)
            .expect("replacement generation should adopt");
        let before_stale_quarantine =
            fs::read(allocator.state_path()).expect("replacement authority should read");
        let error = allocator
            .quarantine(&tenant, &attachment, Some(&original))
            .expect_err("stale generation must not quarantine the replacement");
        assert!(
            error.to_string().contains("adoption receipt")
                && error.to_string().contains("current generation"),
            "the rejection should identify the exact generation fence: {error}"
        );
        assert_eq!(
            fs::read(allocator.state_path()).expect("replacement authority should re-read"),
            before_stale_quarantine,
            "stale quarantine must fail inside the authority transaction without mutation"
        );

        allocator
            .quarantine(&tenant, &attachment, Some(&replacement))
            .expect("the exact replacement generation should quarantine");
        let before_stale_release =
            fs::read(allocator.state_path()).expect("quarantined authority should read");
        let error = allocator
            .release(&tenant, &attachment, Some(&original))
            .expect_err("stale generation must not release the replacement");
        assert!(
            error.to_string().contains("adoption receipt")
                && error.to_string().contains("current generation"),
            "the release rejection should identify the exact generation fence: {error}"
        );
        assert_eq!(
            fs::read(allocator.state_path()).expect("quarantined authority should re-read"),
            before_stale_release,
            "stale release must fail inside the authority transaction without mutation"
        );
        assert!(matches!(
            allocator
                .release(&tenant, &attachment, Some(&replacement))
                .expect("the exact replacement generation should release"),
            NetworkSegmentReleaseOutcome::CleanupPending(_)
        ));
    }

    #[test]
    fn selected_secondary_binding_is_idempotent_and_refuses_remap_across_reopen() {
        let root = tempdir().expect("state root");
        let tenant = tenant();
        let attachment = attachment("secondary-workload");
        let owner = claim("secondary-owner");
        let allocator = SingleNodeSegmentAllocator::single_node_default(root.path());
        allocator
            .reserve_attachment_for_coordinator(&tenant, &attachment, &owner)
            .expect("claim should reserve an unplaced attachment");
        let primary = allocator
            .segments_for(&tenant)
            .expect("primary segment should inspect");
        let secondary = match allocator
            .grow_block_if_current(&tenant, &primary)
            .expect("current primary observation should permit growth")
        {
            NetworkSegmentGrowth::Grown(segment) => segment,
            NetworkSegmentGrowth::ObservationStale => {
                panic!("single-threaded exact observation must remain current")
            }
        };
        allocator
            .bind_reserved_attachment_to_segment(
                &tenant,
                &attachment,
                secondary.segment_id(),
                &owner,
            )
            .expect("placement should bind the selected secondary segment");

        let reopened = SingleNodeSegmentAllocator::single_node_default(root.path());
        let replayed = reopened
            .bind_reserved_attachment_to_segment(
                &tenant,
                &attachment,
                secondary.segment_id(),
                &owner,
            )
            .expect("acknowledgement-loss replay must preserve the exact binding");
        assert_eq!(replayed.segment_id(), secondary.segment_id());
        let remap = reopened
            .bind_reserved_attachment_to_segment(
                &tenant,
                &attachment,
                primary[0].segment_id(),
                &owner,
            )
            .expect_err("an existing exact binding must reject segment remap");
        assert!(
            remap.to_string().contains("refusing remap"),
            "remap rejection should name the durable selected-segment fence: {remap}"
        );
        let adopted = reopened
            .adopt_reserved_attachment(&tenant, &attachment, &owner)
            .expect("the exact owner should adopt the persisted selected segment");
        assert_eq!(
            adopted.segment_id(),
            secondary.segment_id(),
            "reopen and rejected remap must not fall back to the primary segment"
        );
    }

    #[test]
    fn exact_compensation_preserves_an_ordinary_sibling_hold() {
        let root = tempdir().expect("state root");
        let allocator = SingleNodeSegmentAllocator::single_node_default(root.path());
        let tenant = tenant();
        let claimed = attachment("claimed-workload");
        let sibling = attachment("live-sibling");
        let owner = claim("owner");
        let segment = reserve_primary(&allocator, &tenant, &claimed, &owner);
        allocator
            .acquire(&tenant, &sibling)
            .expect("ordinary sibling should acquire");

        assert_eq!(
            allocator
                .release_reserved_attachment_without_effect(&tenant, &claimed, &owner)
                .expect("exact claimed hold should fence IPAM cleanup"),
            NetworkSegmentReleaseOutcome::AttachmentCleanupPending
        );
        let before_foreign =
            std::fs::read(allocator.state_path()).expect("authority should be durable");
        allocator
            .finalize_reserved_attachment_without_effect(
                &tenant,
                &claimed,
                &claim("foreign-finalizer"),
            )
            .expect_err("foreign finalization must not remove pending exact authority");
        assert_eq!(
            std::fs::read(allocator.state_path()).expect("authority should remain durable"),
            before_foreign,
            "foreign IPAM-finalization acknowledgement must not mutate segment authority"
        );
        assert_eq!(
            allocator
                .finalize_reserved_attachment_without_effect(&tenant, &claimed, &owner)
                .expect("exact owner should confirm IPAM cleanup"),
            NetworkSegmentReleaseOutcome::StillLive
        );
        let retained = allocator
            .inspect_segments(&tenant)
            .expect("segment should inspect")
            .expect("sibling hold must retain the tenant allocation");
        assert_eq!(retained[0].segment_id(), segment.segment_id());
        assert!(
            allocator.has_hold(tenant.as_str(), "live-sibling"),
            "sibling authority must remain live"
        );
    }

    #[test]
    fn reserved_segment_cleanup_retry_reconstructs_pending_cleanup_after_finalization_failure() {
        let root = tempdir().expect("state root");
        let tenant = tenant();
        let attachment = attachment("retry-workload");
        let owner = claim("retry-owner");
        let allocator = SingleNodeSegmentAllocator::single_node_default(root.path());
        allocator
            .reserve_attachment_for_coordinator(&tenant, &attachment, &owner)
            .expect("claim should reserve the attachment");
        assert_eq!(
            allocator
                .release_reserved_attachment_without_effect(&tenant, &attachment, &owner)
                .expect("exact compensation should fence IPAM cleanup"),
            NetworkSegmentReleaseOutcome::AttachmentCleanupPending
        );
        let reopened_before_ipam = SingleNodeSegmentAllocator::single_node_default(root.path());
        assert_eq!(
            reopened_before_ipam
                .release_reserved_attachment_without_effect(&tenant, &attachment, &owner)
                .expect("exact retry should preserve pending IPAM cleanup"),
            NetworkSegmentReleaseOutcome::AttachmentCleanupPending
        );
        let cleanup = match reopened_before_ipam
            .finalize_reserved_attachment_without_effect(&tenant, &attachment, &owner)
            .expect("exact IPAM cleanup confirmation should enter allocation cleanup")
        {
            NetworkSegmentReleaseOutcome::CleanupPending(cleanup) => cleanup,
            outcome => panic!("last exact finalization should return cleanup, got {outcome:?}"),
        };

        let rejected = NetworkSegmentCleanup::new(
            tenant.clone(),
            vec![NetworkSegmentId::generate()],
            cleanup.lease_epoch(),
            cleanup.segments().to_vec(),
        );
        allocator
            .finalize_release(&rejected)
            .expect_err("a mismatched finalization proof must fail without releasing the segment");

        let reopened = SingleNodeSegmentAllocator::single_node_default(root.path());
        let before_foreign = std::fs::read(reopened.state_path())
            .expect("authority should read before foreign retry");
        let foreign_error = reopened
            .release_reserved_attachment_without_effect(
                &tenant,
                &attachment,
                &claim("foreign-retry"),
            )
            .expect_err("a foreign claim must not reconstruct pending cleanup");
        assert!(
            foreign_error.to_string().contains("different launch"),
            "foreign retry should identify the reservation fence: {foreign_error}"
        );
        assert_eq!(
            std::fs::read(reopened.state_path())
                .expect("authority should read after rejected foreign retry"),
            before_foreign,
            "foreign retry must leave durable cleanup authority byte-unchanged"
        );
        let retry_cleanup = match reopened
            .release_reserved_attachment_without_effect(&tenant, &attachment, &owner)
            .expect("exact compensation retry should reconstruct pending cleanup")
        {
            NetworkSegmentReleaseOutcome::CleanupPending(cleanup) => cleanup,
            outcome => panic!("retry should return the same pending cleanup, got {outcome:?}"),
        };
        assert_eq!(
            retry_cleanup, cleanup,
            "retry must reconstruct the exact durable identity and epoch fence"
        );
        assert_eq!(
            reopened
                .finalize_release(&retry_cleanup)
                .expect("reconstructed cleanup should finalize"),
            NetworkSegmentFinalizeOutcome::Released
        );
        assert_eq!(
            reopened
                .finalize_release(&retry_cleanup)
                .expect("repeated finalization should be idempotent"),
            NetworkSegmentFinalizeOutcome::AlreadyReleased
        );
    }
}
