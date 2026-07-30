use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use nimbus_core::{Cidr, TenantId};
use nimbus_network::{
    AllocatedSegment, LocalPortLeaseAuthority, NetworkAttachmentId,
    NetworkAttachmentReservationObservation, NetworkAttachmentReservationState,
    NetworkAttachmentSegmentAssociation, NetworkLeaseEpoch, NetworkReservationClaim,
    NetworkSegmentAllocator, NetworkSegmentCleanup, NetworkSegmentFinalizeOutcome,
    NetworkSegmentGrowth, NetworkSegmentQuarantineOutcome, NetworkSegmentReleaseOutcome,
};

use crate::error::SandboxError;

use super::{OciIpamAuthority, OciNetworkLayout, OciSegmentRealization};

pub(crate) fn direct_test_ipam_authority(layout: &OciNetworkLayout) -> OciIpamAuthority {
    OciIpamAuthority::reconstruct_for_direct_test(layout)
        .expect("direct test IPAM authority should open")
}

pub(crate) fn direct_test_port_authority(
    state_root: impl AsRef<std::path::Path>,
) -> LocalPortLeaseAuthority {
    LocalPortLeaseAuthority::open(state_root).expect("direct test port authority should open")
}

type ReserveAttachmentObserver =
    dyn Fn(&NetworkReservationClaim) -> Result<(), SandboxError> + Send + Sync;
type AdoptAttachmentObserver =
    dyn Fn(&NetworkReservationClaim) -> Result<(), SandboxError> + Send + Sync;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SegmentAllocatorOperation {
    SegmentFor(TenantId),
    SegmentsFor(TenantId),
    InspectSegments(TenantId),
    InspectAttachment(TenantId, NetworkAttachmentId),
    ReserveAttachment(TenantId, NetworkAttachmentId),
    BindAttachment(TenantId, NetworkAttachmentId, String),
    AdoptAttachment(TenantId, NetworkAttachmentId),
    ReleaseReservedAttachment(TenantId, NetworkAttachmentId),
    FinalizeReservedAttachment(TenantId, NetworkAttachmentId),
    Acquire(TenantId, NetworkAttachmentId),
    Quarantine(TenantId, NetworkAttachmentId),
    Release(TenantId, NetworkAttachmentId),
    FinalizeRelease(TenantId, Vec<String>),
    GrowBlockIfCurrent(TenantId, Vec<String>),
    Reconcile(BTreeSet<(TenantId, NetworkAttachmentId)>),
}

#[derive(Clone)]
struct RecordingAttachmentReservation {
    claim: NetworkReservationClaim,
    state: NetworkAttachmentReservationState,
    association_bound: bool,
}

/// Behavior-recording substitute for proving OCI backends consume only the
/// portable allocator capability.
pub(crate) struct RecordingSegmentAllocator {
    segment: OciSegmentRealization,
    operations: Arc<Mutex<Vec<SegmentAllocatorOperation>>>,
    quarantine_failure: Option<String>,
    release_reserved_failure: Option<String>,
    finalize_release_failure: Arc<Mutex<Option<String>>>,
    reserve_attachment_observer: Option<Arc<ReserveAttachmentObserver>>,
    adopt_attachment_observer: Option<Arc<AdoptAttachmentObserver>>,
    attachment_reservation: Arc<Mutex<Option<RecordingAttachmentReservation>>>,
    observed_association_override: Arc<Mutex<Option<NetworkAttachmentSegmentAssociation>>>,
}

impl RecordingSegmentAllocator {
    pub(crate) fn new(tenant: TenantId, cidr: &str, local_slot: u32) -> Self {
        let allocation = AllocatedSegment::new(
            "netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV"
                .parse()
                .expect("recording segment ID should parse"),
            tenant,
            Cidr::parse(cidr).expect("recording segment CIDR should parse"),
            NetworkLeaseEpoch::new(41),
        );
        Self {
            segment: OciSegmentRealization::from_local_slot(allocation, local_slot),
            operations: Arc::new(Mutex::new(Vec::new())),
            quarantine_failure: None,
            release_reserved_failure: None,
            finalize_release_failure: Arc::new(Mutex::new(None)),
            reserve_attachment_observer: None,
            adopt_attachment_observer: None,
            attachment_reservation: Arc::new(Mutex::new(None)),
            observed_association_override: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn with_quarantine_failure(mut self, message: impl Into<String>) -> Self {
        self.quarantine_failure = Some(message.into());
        self
    }

    pub(crate) fn with_reserve_attachment_observer(
        mut self,
        observer: impl Fn(&NetworkReservationClaim) -> Result<(), SandboxError> + Send + Sync + 'static,
    ) -> Self {
        self.reserve_attachment_observer = Some(Arc::new(observer));
        self
    }

    pub(crate) fn with_release_reserved_failure(mut self, message: impl Into<String>) -> Self {
        self.release_reserved_failure = Some(message.into());
        self
    }

    pub(crate) fn with_finalize_release_failure(self, message: impl Into<String>) -> Self {
        *self
            .finalize_release_failure
            .lock()
            .expect("recording allocator failure lock should not be poisoned") =
            Some(message.into());
        self
    }

    pub(crate) fn clear_finalize_release_failure(&self) {
        *self
            .finalize_release_failure
            .lock()
            .expect("recording allocator failure lock should not be poisoned") = None;
    }

    pub(crate) fn with_adopt_attachment_observer(
        mut self,
        observer: impl Fn(&NetworkReservationClaim) -> Result<(), SandboxError> + Send + Sync + 'static,
    ) -> Self {
        self.adopt_attachment_observer = Some(Arc::new(observer));
        self
    }

    pub(crate) fn operations(&self) -> Vec<SegmentAllocatorOperation> {
        self.operations
            .lock()
            .expect("recording allocator lock should not be poisoned")
            .clone()
    }

    pub(crate) fn substitute_observed_association_for_test(
        &self,
        association: NetworkAttachmentSegmentAssociation,
    ) {
        *self
            .observed_association_override
            .lock()
            .expect("recording allocator association override lock should not be poisoned") =
            Some(association);
    }

    fn record(&self, operation: SegmentAllocatorOperation) {
        self.operations
            .lock()
            .expect("recording allocator lock should not be poisoned")
            .push(operation);
    }

    fn set_attachment_reservation(
        &self,
        claim: &NetworkReservationClaim,
        state: NetworkAttachmentReservationState,
        association_bound: Option<bool>,
    ) -> Result<(), SandboxError> {
        let mut current = self
            .attachment_reservation
            .lock()
            .expect("recording allocator reservation lock should not be poisoned");
        if current
            .as_ref()
            .is_some_and(|existing| existing.claim != *claim)
        {
            return Err(SandboxError::OperationFailed {
                message: "recording attachment belongs to another reservation claim".to_owned(),
            });
        }
        *current = Some(RecordingAttachmentReservation {
            claim: claim.clone(),
            state,
            association_bound: association_bound
                .or_else(|| current.as_ref().map(|existing| existing.association_bound))
                .unwrap_or(false),
        });
        Ok(())
    }
}

impl NetworkSegmentAllocator for RecordingSegmentAllocator {
    type Segment = OciSegmentRealization;
    type Error = SandboxError;

    fn segment_for(&self, tenant: &TenantId) -> Result<Self::Segment, Self::Error> {
        self.record(SegmentAllocatorOperation::SegmentFor(tenant.clone()));
        Ok(self.segment.clone())
    }

    fn segments_for(&self, tenant: &TenantId) -> Result<Vec<Self::Segment>, Self::Error> {
        self.record(SegmentAllocatorOperation::SegmentsFor(tenant.clone()));
        Ok(vec![self.segment.clone()])
    }

    fn inspect_segments(
        &self,
        tenant: &TenantId,
    ) -> Result<Option<Vec<Self::Segment>>, Self::Error> {
        self.record(SegmentAllocatorOperation::InspectSegments(tenant.clone()));
        Ok(Some(vec![self.segment.clone()]))
    }

    fn inspect_attachment_reservation(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkAttachmentReservationObservation, Self::Error> {
        self.record(SegmentAllocatorOperation::InspectAttachment(
            tenant.clone(),
            attachment_id.clone(),
        ));
        let current = self
            .attachment_reservation
            .lock()
            .expect("recording allocator reservation lock should not be poisoned");
        match current.as_ref() {
            Some(existing) if existing.claim == *reservation_claim => {
                let association = || {
                    self.observed_association_override
                        .lock()
                        .expect(
                            "recording allocator association override lock should not be poisoned",
                        )
                        .clone()
                        .unwrap_or_else(|| {
                            NetworkAttachmentSegmentAssociation::new(
                                existing.claim.clone(),
                                self.segment.segment_id().clone(),
                                self.segment.lease_epoch(),
                            )
                        })
                };
                Ok(match (existing.state, existing.association_bound) {
                    (NetworkAttachmentReservationState::Absent, _) => {
                        NetworkAttachmentReservationObservation::absent()
                    }
                    (NetworkAttachmentReservationState::Reserved, false) => {
                        NetworkAttachmentReservationObservation::unplaced_reserved()
                    }
                    (NetworkAttachmentReservationState::Reserved, true) => {
                        NetworkAttachmentReservationObservation::bound_reserved(association())
                    }
                    (NetworkAttachmentReservationState::ReservationCleanupPending, false) => {
                        NetworkAttachmentReservationObservation::unplaced_cleanup_pending()
                    }
                    (NetworkAttachmentReservationState::ReservationCleanupPending, true) => {
                        NetworkAttachmentReservationObservation::bound_cleanup_pending(association())
                    }
                    (NetworkAttachmentReservationState::Adopted, true) => {
                        NetworkAttachmentReservationObservation::adopted(association())
                    }
                    (NetworkAttachmentReservationState::ProviderCleanupPending, true) => {
                        NetworkAttachmentReservationObservation::provider_cleanup_pending(
                            association(),
                        )
                    }
                    (state, bound) => {
                        return Err(SandboxError::OperationFailed {
                            message: format!(
                                "recording attachment contains invalid state/association pair \
                                 {state:?}/{bound}"
                            ),
                        });
                    }
                })
            }
            Some(_) => Err(SandboxError::OperationFailed {
                message: "recording attachment belongs to another reservation claim".to_owned(),
            }),
            None => Ok(NetworkAttachmentReservationObservation::absent()),
        }
    }

    fn reserve_attachment_for_coordinator(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<(), Self::Error> {
        self.record(SegmentAllocatorOperation::ReserveAttachment(
            tenant.clone(),
            attachment_id.clone(),
        ));
        if let Some(observer) = self.reserve_attachment_observer.as_ref() {
            observer(reservation_claim)?;
        }
        self.set_attachment_reservation(
            reservation_claim,
            NetworkAttachmentReservationState::Reserved,
            Some(false),
        )?;
        Ok(())
    }

    fn bind_reserved_attachment_to_segment(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        segment_id: &nimbus_network::NetworkSegmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<Self::Segment, Self::Error> {
        self.record(SegmentAllocatorOperation::BindAttachment(
            tenant.clone(),
            attachment_id.clone(),
            segment_id.as_str().to_owned(),
        ));
        if segment_id != self.segment.segment_id() {
            return Err(SandboxError::OperationFailed {
                message: format!("recording attachment cannot bind unknown segment {segment_id}"),
            });
        }
        self.set_attachment_reservation(
            reservation_claim,
            NetworkAttachmentReservationState::Reserved,
            Some(true),
        )?;
        Ok(self.segment.clone())
    }

    fn adopt_reserved_attachment(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<Self::Segment, Self::Error> {
        if let Some(observer) = self.adopt_attachment_observer.as_ref() {
            observer(reservation_claim)?;
        }
        self.set_attachment_reservation(
            reservation_claim,
            NetworkAttachmentReservationState::Adopted,
            Some(true),
        )?;
        self.record(SegmentAllocatorOperation::AdoptAttachment(
            tenant.clone(),
            attachment_id.clone(),
        ));
        Ok(self.segment.clone())
    }

    fn release_reserved_attachment_without_effect(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkSegmentReleaseOutcome<Self::Segment>, Self::Error> {
        self.record(SegmentAllocatorOperation::ReleaseReservedAttachment(
            tenant.clone(),
            attachment_id.clone(),
        ));
        if let Some(message) = self.release_reserved_failure.as_ref() {
            return Err(SandboxError::OperationFailed {
                message: message.clone(),
            });
        }
        self.set_attachment_reservation(
            reservation_claim,
            NetworkAttachmentReservationState::ReservationCleanupPending,
            None,
        )?;
        Ok(NetworkSegmentReleaseOutcome::CleanupPending(
            NetworkSegmentCleanup::new(
                tenant.clone(),
                vec![self.segment.segment_id().clone()],
                self.segment.lease_epoch(),
                vec![self.segment.clone()],
            ),
        ))
    }

    fn finalize_reserved_attachment_without_effect(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkSegmentReleaseOutcome<Self::Segment>, Self::Error> {
        self.record(SegmentAllocatorOperation::FinalizeReservedAttachment(
            tenant.clone(),
            attachment_id.clone(),
        ));
        {
            let mut current = self
                .attachment_reservation
                .lock()
                .expect("recording allocator reservation lock should not be poisoned");
            match current.as_ref() {
                Some(existing) if existing.claim == *reservation_claim => *current = None,
                Some(_) => {
                    return Err(SandboxError::OperationFailed {
                        message: "recording attachment belongs to another reservation claim"
                            .to_owned(),
                    });
                }
                None => {}
            }
        }
        Ok(NetworkSegmentReleaseOutcome::CleanupPending(
            NetworkSegmentCleanup::new(
                tenant.clone(),
                vec![self.segment.segment_id().clone()],
                self.segment.lease_epoch(),
                vec![self.segment.clone()],
            ),
        ))
    }

    fn acquire(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<Self::Segment, Self::Error> {
        self.record(SegmentAllocatorOperation::Acquire(
            tenant.clone(),
            attachment_id.clone(),
        ));
        Ok(self.segment.clone())
    }

    fn quarantine(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        expected_adoption_receipt: Option<&NetworkReservationClaim>,
    ) -> Result<NetworkSegmentQuarantineOutcome, Self::Error> {
        self.record(SegmentAllocatorOperation::Quarantine(
            tenant.clone(),
            attachment_id.clone(),
        ));
        if let Some(message) = self.quarantine_failure.as_ref() {
            return Err(SandboxError::OperationFailed {
                message: message.clone(),
            });
        }
        if let Some(claim) = expected_adoption_receipt {
            self.set_attachment_reservation(
                claim,
                NetworkAttachmentReservationState::ProviderCleanupPending,
                Some(true),
            )?;
        }
        Ok(NetworkSegmentQuarantineOutcome::CleanupPending)
    }

    fn release(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
        expected_adoption_receipt: Option<&NetworkReservationClaim>,
    ) -> Result<NetworkSegmentReleaseOutcome<Self::Segment>, Self::Error> {
        self.record(SegmentAllocatorOperation::Release(
            tenant.clone(),
            attachment_id.clone(),
        ));
        if let Some(claim) = expected_adoption_receipt {
            self.set_attachment_reservation(
                claim,
                NetworkAttachmentReservationState::ProviderCleanupPending,
                Some(true),
            )?;
        }
        Ok(NetworkSegmentReleaseOutcome::CleanupPending(
            NetworkSegmentCleanup::new(
                tenant.clone(),
                vec![self.segment.segment_id().clone()],
                self.segment.lease_epoch(),
                vec![self.segment.clone()],
            ),
        ))
    }

    fn finalize_release(
        &self,
        cleanup: &NetworkSegmentCleanup<Self::Segment>,
    ) -> Result<NetworkSegmentFinalizeOutcome, Self::Error> {
        self.record(SegmentAllocatorOperation::FinalizeRelease(
            cleanup.tenant_id().clone(),
            cleanup
                .segment_ids()
                .iter()
                .map(|segment_id| segment_id.as_str().to_owned())
                .collect(),
        ));
        if let Some(message) = self
            .finalize_release_failure
            .lock()
            .expect("recording allocator failure lock should not be poisoned")
            .as_ref()
        {
            return Err(SandboxError::OperationFailed {
                message: message.clone(),
            });
        }
        let mut current = self
            .attachment_reservation
            .lock()
            .expect("recording allocator reservation lock should not be poisoned");
        if current.as_ref().is_some_and(|reservation| {
            reservation.state == NetworkAttachmentReservationState::ProviderCleanupPending
        }) {
            *current = None;
        }
        Ok(NetworkSegmentFinalizeOutcome::Released)
    }

    fn grow_block_if_current(
        &self,
        tenant: &TenantId,
        observed_segments: &[Self::Segment],
    ) -> Result<NetworkSegmentGrowth<Self::Segment>, Self::Error> {
        self.record(SegmentAllocatorOperation::GrowBlockIfCurrent(
            tenant.clone(),
            observed_segments
                .iter()
                .map(|segment| segment.segment_id().as_str().to_owned())
                .collect(),
        ));
        Ok(NetworkSegmentGrowth::Grown(self.segment.clone()))
    }

    fn reconcile_orphans(
        &self,
        live: &BTreeSet<(TenantId, NetworkAttachmentId)>,
    ) -> Result<Vec<Self::Segment>, Self::Error> {
        self.record(SegmentAllocatorOperation::Reconcile(live.clone()));
        Ok(Vec::new())
    }
}
